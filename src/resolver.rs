//! A radio channel is a hint. Only an authenticated host/repository can refresh it.
use crate::endpoint::{Channel, Endpoint};
use anyhow::{Result, bail};
use std::{future::Future, time::Duration};
use tokio::time::{Instant, timeout, timeout_at};

const CACHED_TIMEOUT: Duration = Duration::from_secs(45);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
const CANDIDATE_TIMEOUT: Duration = Duration::from_secs(15);
const SEARCH_TIMEOUT: Duration = Duration::from_secs(120);

/// Refresh advertised contacts before dialing a potentially stale channel.
/// `dial` must authenticate both the pinned host key and the requested share ID.
pub(crate) async fn resolve<T, F, Fut, D>(
    endpoint: &Endpoint,
    discovery: D,
    mut dial: F,
) -> Result<(T, Endpoint)>
where
    F: FnMut(Endpoint, bool) -> Fut,
    Fut: Future<Output = Result<T>>,
    D: Future<Output = Vec<Channel>>,
{
    let hints = timeout(DISCOVERY_TIMEOUT, discovery)
        .await
        .unwrap_or_default();
    let advertised = !hints.is_empty();
    let channels = if advertised {
        hints
    } else {
        std::iter::once(endpoint.channel)
            .chain((1..=30).map(|number| Channel::try_from(number).expect("valid channel range")))
            .collect()
    };
    let mut tried = [false; 31];
    let channels: Vec<_> = channels
        .into_iter()
        .filter(|channel| !std::mem::replace(&mut tried[usize::from(channel.get())], true))
        .collect();
    let first = Endpoint {
        channel: channels[0],
        ..endpoint.clone()
    };
    let initial_error = match timeout(CACHED_TIMEOUT, dial(first.clone(), false)).await {
        Ok(Ok(session)) => return Ok((session, first)),
        Ok(Err(error)) => error,
        Err(error) => anyhow::Error::from(error).context("initial connection timed out"),
    };
    let diagnostics = std::env::var_os("BLUGIT_BLUETOOTH_DIAGNOSTICS").is_some();
    if diagnostics {
        eprintln!(
            "blugit: initial RFCOMM channel {}: {initial_error:#}",
            first.channel.get()
        );
    }
    // A legacy loopback test listener has no channel routing. Production never
    // takes this branch; the routed test transport below exercises rediscovery.
    #[cfg(feature = "test-transport")]
    if std::env::var_os("BLUGIT_TEST_ADDR").is_some()
        && std::env::var_os("BLUGIT_TEST_ROUTES").is_none()
    {
        return Err(initial_error);
    }
    eprintln!("blugit: locating the registered repository on the known host...");
    let deadline = Instant::now() + SEARCH_TIMEOUT;
    for channel in channels.into_iter().skip(1) {
        let number = channel.get();
        let candidate = Endpoint {
            channel,
            ..endpoint.clone()
        };
        let error = match timeout_at(
            deadline,
            timeout(
                if advertised {
                    CACHED_TIMEOUT
                } else {
                    CANDIDATE_TIMEOUT
                },
                dial(candidate.clone(), !advertised),
            ),
        )
        .await
        {
            Ok(Ok(Ok(session))) => return Ok((session, candidate)),
            Ok(Ok(Err(error))) => error,
            Ok(Err(error)) => anyhow::Error::from(error).context("candidate timed out"),
            Err(_) => bail!(
                "timed out locating the registered repository; first connection: {initial_error:#}"
            ),
        };
        if diagnostics {
            eprintln!("blugit: RFCOMM channel {number}: {error:#}");
        }
    }
    bail!(
        "registered repository could not be authenticated on the candidate RFCOMM channels; first connection: {initial_error:#}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ShareId;
    use std::sync::{Arc, Mutex};
    fn endpoint() -> Endpoint {
        Endpoint {
            address: "AA:BB:CC:DD:EE:FF".parse().unwrap(),
            channel: Channel::try_from(7).unwrap(),
            share: ShareId::generate(),
        }
    }
    #[tokio::test]
    async fn cached_connection_does_not_probe_other_channels() -> Result<()> {
        let endpoint = endpoint();
        let (value, resolved) = resolve(
            &endpoint,
            std::future::ready(Vec::new()),
            |candidate, probing| async move {
                assert!(!probing);
                assert_eq!(candidate.channel.get(), 7);
                Ok(42)
            },
        )
        .await?;
        assert_eq!(value, 42);
        assert_eq!(resolved, endpoint);
        Ok(())
    }
    #[tokio::test]
    async fn search_preserves_identity_and_updates_only_after_authentication() -> Result<()> {
        let original = endpoint();
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let (value, resolved) = resolve(
            &original,
            std::future::ready(Vec::new()),
            |candidate, probing| {
                let attempts = attempts.clone();
                let original = original.clone();
                async move {
                    assert_eq!(candidate.address, original.address);
                    assert_eq!(candidate.share, original.share);
                    let number = candidate.channel.get();
                    attempts.lock().unwrap().push(number);
                    assert_eq!(probing, number != 7);
                    // Reachable but unauthenticated/wrong-share candidates must not win.
                    if number != 4 {
                        bail!("not the pinned host and repository");
                    }
                    Ok("authenticated session")
                }
            },
        )
        .await?;
        assert_eq!(value, "authenticated session");
        assert_eq!(resolved.channel.get(), 4);
        assert_eq!(*attempts.lock().unwrap(), [7, 1, 2, 3, 4]);
        Ok(())
    }
    #[tokio::test]
    async fn advertised_candidates_are_prioritized_but_still_require_authentication() -> Result<()>
    {
        let original = endpoint();
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let hints = [3, 3, 7, 29]
            .map(|number| Channel::try_from(number).unwrap())
            .to_vec();
        let (_, resolved) = resolve(&original, std::future::ready(hints), |candidate, _| {
            let attempts = attempts.clone();
            let original = original.clone();
            async move {
                assert_eq!(candidate.address, original.address);
                assert_eq!(candidate.share, original.share);
                let channel = candidate.channel.get();
                attempts.lock().unwrap().push(channel);
                if channel != 29 {
                    bail!("advertised service has the wrong host key or share");
                }
                Ok(())
            }
        })
        .await?;
        assert_eq!(*attempts.lock().unwrap(), [3, 7, 29]);
        assert_eq!(resolved.channel.get(), 29);
        Ok(())
    }
    #[tokio::test]
    async fn fresh_advert_bypasses_the_stale_cache_and_never_scans_unadvertised_ports() -> Result<()>
    {
        let original = endpoint();
        let hints = vec![Channel::try_from(10)?];
        let (_, resolved) = resolve(
            &original,
            std::future::ready(hints),
            |candidate, probing| async move {
                assert_eq!(
                    candidate.channel.get(),
                    10,
                    "stale port must not be dialed first"
                );
                assert!(!probing);
                Ok(())
            },
        )
        .await?;
        assert_eq!(resolved.channel.get(), 10);
        let result = resolve::<(), _, _, _>(
            &original,
            std::future::ready(vec![Channel::try_from(10)?]),
            |candidate, _| async move {
                assert_eq!(
                    candidate.channel.get(),
                    10,
                    "do not disturb unrelated RFCOMM services"
                );
                bail!("wrong pinned host key");
            },
        )
        .await;
        assert!(format!("{:#}", result.unwrap_err()).contains("wrong pinned host key"));
        Ok(())
    }
    #[tokio::test(start_paused = true)]
    async fn unresponsive_channels_have_a_bounded_search_budget() {
        let start = Instant::now();
        let result = resolve::<(), _, _, _>(&endpoint(), std::future::ready(Vec::new()), |_, _| {
            std::future::pending::<Result<()>>()
        })
        .await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("timed out locating")
        );
        assert_eq!(Instant::now() - start, CACHED_TIMEOUT + SEARCH_TIMEOUT);
    }
    #[tokio::test]
    async fn failed_search_tries_each_channel_once_and_preserves_original_reason() {
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let result = resolve::<(), _, _, _>(
            &endpoint(),
            std::future::ready(Vec::new()),
            |candidate, _| {
                let attempts = attempts.clone();
                async move {
                    attempts.lock().unwrap().push(candidate.channel.get());
                    bail!("host key changed");
                }
            },
        )
        .await;
        assert!(format!("{:#}", result.unwrap_err()).contains("host key changed"));
        let mut channels = attempts.lock().unwrap().clone();
        channels.sort();
        assert_eq!(channels, (1..=30).collect::<Vec<_>>());
    }
}
