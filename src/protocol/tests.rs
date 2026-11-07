use super::*;
use crate::{endpoint::Channel, test_support::BufferedBridge};

struct Fixture {
    _dir: tempfile::TempDir,
    host: Device,
    guest: Device,
    share: ShareStore,
    endpoint: Endpoint,
}

#[tokio::test]
async fn enrollment_learns_share_and_pins_it_only_after_code_authentication() -> Result<()> {
    let f = Fixture::new()?;
    let (server, mut codes) = f.server();
    let (a, b) = tokio::io::duplex(67);
    let trust = TrustStore::new(f.guest.root().into());
    let (host, guest) = tokio::join!(
        server.accept(Box::new(a)),
        enroll_at(
            Box::new(b),
            &f.guest,
            f.endpoint.contact(),
            None,
            |_| async {
                assert!(trust.host(&f.endpoint)?.is_none());
                codes.recv().await.context("no code")
            }
        )
    );
    host?;
    let (endpoint, host) = guest?;
    assert_eq!(endpoint.share, f.share.id());
    assert_eq!(endpoint.address, f.endpoint.address);
    assert_eq!(host.key, f.host.peer().key);
    assert_eq!(trust.host(&endpoint)?.unwrap().key, host.key);
    assert_eq!(
        trust.host(&endpoint)?.unwrap().ssh_key,
        f.host.peer().ssh_key
    );
    assert_eq!(
        f.share.member(&f.guest.peer().key)?.unwrap().ssh_key,
        f.guest.peer().ssh_key
    );
    Ok(())
}

#[tokio::test]
async fn modified_share_or_ssh_keys_fail_authentication_and_are_not_saved() -> Result<()> {
    for tamper in ["share", "host_ssh", "guest_ssh"] {
        let f = Fixture::new()?;
        let bogus_share = ShareId::generate();
        let (server, mut codes) = f.server();
        let (a, relay_host) = tokio::io::duplex(4096);
        let (relay_guest, b) = tokio::io::duplex(4096);
        let relay = async {
            let mut relay_host: Connection = Box::new(relay_host);
            let mut relay_guest: Connection = Box::new(relay_guest);
            let mut magic = vec![0; MAGIC.len()];
            relay_guest.read_exact(&mut magic).await?;
            relay_host.write_all(&magic).await?;
            let mut request: Request = framing::receive_json(&mut relay_guest).await?;
            if tamper == "guest_ssh" {
                request.peer.ssh_key = [42; 32];
            }
            framing::send_json(&mut relay_host, &request).await?;
            let mut response: Response = framing::receive_json(&mut relay_host).await?;
            if let Response::Ready { reply } = &mut response {
                if tamper == "share" {
                    reply.share = bogus_share;
                }
                if tamper == "host_ssh" {
                    reply.host.ssh_key = [42; 32];
                }
            }
            framing::send_json(&mut relay_guest, &response).await?;
            let _ = tokio::io::copy_bidirectional(&mut relay_host, &mut relay_guest).await;
            Ok::<_, anyhow::Error>(())
        };
        let (host, guest, relay) = timeout(Duration::from_secs(5), async {
            tokio::join!(
                server.accept(Box::new(a)),
                enroll_at(
                    Box::new(b),
                    &f.guest,
                    f.endpoint.contact(),
                    None,
                    |_| async { codes.recv().await.context("no code") }
                ),
                relay
            )
        })
        .await?;
        relay?;
        assert!(host.is_err() && guest.is_err());
        assert!(f.share.members()?.is_empty());
        let trust = TrustStore::new(f.guest.root().into());
        assert!(trust.host(&f.endpoint)?.is_none());
        let mut bogus = f.endpoint.clone();
        bogus.share = bogus_share;
        assert!(trust.host(&bogus)?.is_none());
    }
    Ok(())
}

impl Fixture {
    fn new() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let repo = dir.path().join("repo.git");
        std::fs::create_dir(&repo)?;
        let host = Device::open(dir.path().join("host"), Some("Host".into()))?;
        let guest = Device::open(dir.path().join("guest"), Some("Guest".into()))?;
        let share = ShareStore::register(host.root().into(), &repo)?;
        let endpoint = Endpoint {
            address: "AA:BB:CC:DD:EE:FF".parse()?,
            share: share.id(),
            channel: Channel::try_from(7)?,
        };
        Ok(Self {
            _dir: dir,
            host,
            guest,
            share,
            endpoint,
        })
    }
    fn server(&self) -> (Server, tokio::sync::mpsc::UnboundedReceiver<Code>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (
            Server::new(
                self.host.clone(),
                self.share.clone(),
                Arc::new(move |notice| {
                    let _ = tx.send(notice.code);
                }),
            ),
            rx,
        )
    }
    async fn register(&self, wrong: bool) -> Result<()> {
        let (server, mut codes) = self.server();
        let (a, b) = tokio::io::duplex(67);
        let (host, guest) = timeout(Duration::from_secs(5), async {
            tokio::join!(
                server.accept(Box::new(a)),
                enroll(Box::new(b), &self.guest, &self.endpoint, |_| async move {
                    let code = codes.recv().await.context("no host code")?;
                    if wrong {
                        Code::parse(&format!(
                            "{:06}",
                            (code.as_str().parse::<u32>()? + 1) % 1_000_000
                        ))
                    } else {
                        Ok(code)
                    }
                })
            )
        })
        .await?;
        if wrong {
            ensure!(host.is_err() && guest.is_err(), "wrong code accepted");
        } else {
            host?;
            ensure!(guest?.key == self.host.peer().key, "wrong host pinned");
        }
        Ok(())
    }
}

#[tokio::test]
async fn wrong_code_never_pins_either_identity() -> Result<()> {
    let f = Fixture::new()?;
    f.register(true).await?;
    assert!(f.share.members()?.is_empty());
    assert!(
        TrustStore::new(f.guest.root().into())
            .host(&f.endpoint)?
            .is_none()
    );
    Ok(())
}
#[tokio::test]
async fn wrong_repository_never_issues_a_code_or_grants_access() -> Result<()> {
    let f = Fixture::new()?;
    let (server, mut codes) = f.server();
    let (a, b) = tokio::io::duplex(4096);
    let mut wrong = f.endpoint.clone();
    wrong.share = ShareId::generate();
    let (host, guest) = tokio::join!(
        server.accept(Box::new(a)),
        enroll(Box::new(b), &f.guest, &wrong, |_| async {
            bail!("must not ask for a code")
        })
    );
    assert!(host.is_err());
    assert!(guest.is_err());
    assert!(codes.try_recv().is_err());
    assert!(f.share.members()?.is_empty());
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn expiry_and_cancellation_do_not_create_permissions() -> Result<()> {
    let f = Fixture::new()?;
    let (mut server, mut codes) = f.server();
    server.gate.lifetime = Duration::from_secs(3);
    let (a, b) = tokio::io::duplex(4096);
    let (host, guest) = tokio::join!(
        server.accept(Box::new(a)),
        enroll(Box::new(b), &f.guest, &f.endpoint, |_| async move {
            let code = codes.recv().await.context("no code")?;
            tokio::time::sleep(Duration::from_secs(4)).await;
            Ok(code)
        })
    );
    assert!(host.is_err());
    assert!(guest.is_err());
    assert!(f.share.members()?.is_empty());
    let (server, _codes) = f.server();
    let (a, b) = tokio::io::duplex(4096);
    let (host, guest) = tokio::join!(
        server.accept(Box::new(a)),
        timeout(
            Duration::from_secs(1),
            enroll(
                Box::new(b),
                &f.guest,
                &f.endpoint,
                |_| std::future::pending::<Result<Code>>()
            )
        )
    );
    assert!(host.is_err());
    assert!(guest.is_err());
    assert!(f.share.members()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn registration_flushes_the_buffered_bridge_before_returning() -> Result<()> {
    let f = Fixture::new()?;
    let (server, mut codes) = f.server();
    let (a, b) = tokio::io::duplex(4096);
    let (host, guest) = timeout(Duration::from_secs(5), async {
        tokio::join!(
            server.accept(Box::new(a)),
            enroll(
                BufferedBridge::wrap(b),
                &f.guest,
                &f.endpoint,
                |_| async move { codes.recv().await.context("no code") }
            )
        )
    })
    .await?;
    host?;
    assert_eq!(guest?.key, f.host.peer().key);
    assert!(f.share.member(&f.guest.peer().key)?.is_some());
    Ok(())
}
