//! A radio channel is a hint. Only an authenticated host/repository can refresh it.
use crate::endpoint::{Channel, Endpoint};
use anyhow::{Result, bail};
use std::{future::Future, time::Duration};
use tokio::time::{Instant, timeout, timeout_at};

const CACHED_TIMEOUT: Duration = Duration::from_secs(45);
const CANDIDATE_TIMEOUT: Duration = Duration::from_secs(15);
const SEARCH_TIMEOUT: Duration = Duration::from_secs(120);

/// Try the cached contact, then search only the already-known Bluetooth address.
/// `dial` must authenticate both the pinned host key and the requested share ID.
pub(crate) async fn resolve<T, F, Fut>(endpoint: &Endpoint, mut dial: F) -> Result<(T, Endpoint)>
where
    F: FnMut(Endpoint, bool) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let initial_error = match timeout(CACHED_TIMEOUT, dial(endpoint.clone(), false)).await {
        Ok(Ok(session)) => return Ok((session, endpoint.clone())),
        Ok(Err(error)) => error,
        Err(error) => anyhow::Error::from(error).context("cached connection timed out"),
    };
    let diagnostics = std::env::var_os("BLUGIT_BLUETOOTH_DIAGNOSTICS").is_some();
    if diagnostics {
        eprintln!(
            "blugit: cached RFCOMM channel {}: {initial_error:#}",
            endpoint.channel.get()
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
    for number in 1..=30 {
        let channel = Channel::try_from(number)?;
        if channel == endpoint.channel {
            continue;
        }
        let candidate = Endpoint {
            channel,
            ..endpoint.clone()
        };
        let error = match timeout_at(
            deadline,
            timeout(CANDIDATE_TIMEOUT, dial(candidate.clone(), true)),
        )
        .await
        {
            Ok(Ok(Ok(session))) => return Ok((session, candidate)),
            Ok(Ok(Err(error))) => error,
            Ok(Err(error)) => anyhow::Error::from(error).context("candidate timed out"),
            Err(_) => bail!(
                "timed out locating the registered repository; cached connection: {initial_error:#}"
            ),
        };
        if diagnostics {
            eprintln!("blugit: RFCOMM channel {number}: {error:#}");
        }
    }
    bail!(
        "registered repository could not be authenticated on any RFCOMM channel; cached connection: {initial_error:#}"
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
        let (value, resolved) = resolve(&endpoint, |candidate, probing| async move {
            assert!(!probing);
            assert_eq!(candidate.channel.get(), 7);
            Ok(42)
        })
        .await?;
        assert_eq!(value, 42);
        assert_eq!(resolved, endpoint);
        Ok(())
    }
    #[tokio::test]
    async fn search_preserves_identity_and_updates_only_after_authentication() -> Result<()> {
        let original = endpoint();
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let (value, resolved) = resolve(&original, |candidate, probing| {
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
        })
        .await?;
        assert_eq!(value, "authenticated session");
        assert_eq!(resolved.channel.get(), 4);
        assert_eq!(*attempts.lock().unwrap(), [7, 1, 2, 3, 4]);
        Ok(())
    }
    #[tokio::test(start_paused = true)]
    async fn unresponsive_channels_have_a_bounded_search_budget() {
        let start = Instant::now();
        let result =
            resolve::<(), _, _>(&endpoint(), |_, _| std::future::pending::<Result<()>>()).await;
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
        let result = resolve::<(), _, _>(&endpoint(), |candidate, _| {
            let attempts = attempts.clone();
            async move {
                attempts.lock().unwrap().push(candidate.channel.get());
                bail!("host key changed");
            }
        })
        .await;
        assert!(format!("{:#}", result.unwrap_err()).contains("host key changed"));
        let mut channels = attempts.lock().unwrap().clone();
        channels.sort();
        assert_eq!(channels, (1..=30).collect::<Vec<_>>());
    }
}
