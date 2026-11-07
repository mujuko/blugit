use super::*;
use crate::endpoint::Channel;
use std::{
    pin::Pin,
    task::{Context as TaskContext, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};

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
    assert!(host?.is_none());
    let (endpoint, host) = guest?;
    assert_eq!(endpoint.share, f.share.id());
    assert_eq!(endpoint.address, f.endpoint.address);
    assert_eq!(host.key, f.host.peer().key);
    assert_eq!(trust.host(&endpoint)?.unwrap().key, host.key);
    Ok(())
}

#[tokio::test]
async fn modified_discovered_share_fails_authentication_and_is_not_saved() -> Result<()> {
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
        let request: Request = framing::receive_json(&mut relay_guest).await?;
        framing::send_json(&mut relay_host, &request).await?;
        let mut response: Response = framing::receive_json(&mut relay_host).await?;
        if let Response::Ready { reply } = &mut response {
            reply.share = bogus_share;
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
    Ok(())
}

#[tokio::test]
async fn git_connection_without_a_share_id_is_rejected_before_code_issuance() -> Result<()> {
    let f = Fixture::new()?;
    let (server, mut codes) = f.server();
    let (a, b) = tokio::io::duplex(4096);
    let guest = async {
        let mut b: Connection = Box::new(b);
        request(
            &mut b,
            &Request {
                operation: Operation::Connect,
                share: None,
                peer: f.guest.peer().clone(),
            },
        )
        .await?;
        reply(&mut b, None).await
    };
    let (host, guest) = tokio::join!(server.accept(Box::new(a)), guest);
    assert!(host.is_err() && guest.is_err());
    assert!(codes.try_recv().is_err());
    Ok(())
}

#[tokio::test]
async fn revocation_during_key_exchange_denies_the_new_session() -> Result<()> {
    let f = Fixture::new()?;
    f.register(false).await?;
    let (server, _) = f.server();
    let (a, b) = tokio::io::duplex(4096);
    let guest = async {
        let mut b: Connection = Box::new(b);
        let r = Request {
            operation: Operation::Connect,
            share: Some(f.share.id()),
            peer: f.guest.peer().clone(),
        };
        request(&mut b, &r).await?;
        let response = reply(&mut b, Some(f.endpoint.share)).await?;
        f.share.revoke(&f.guest.peer().short_id())?;
        let transcript = messages::transcript(&r, &response)?;
        let mut secure =
            handshake::exchange(b, &f.guest, &response.host, &transcript, None, true).await?;
        let result: Access = framing::receive_json(&mut secure).await?;
        ensure!(
            matches!(result, Access::Denied),
            "revoked device was granted access"
        );
        Ok::<(), anyhow::Error>(())
    };
    let (host, guest) = tokio::join!(server.accept(Box::new(a)), guest);
    assert!(host.is_err());
    guest?;
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
            ensure!(host?.is_none(), "registration unexpectedly opened Git");
            ensure!(guest?.key == self.host.peer().key, "wrong host pinned");
        }
        Ok(())
    }
}

#[tokio::test]
async fn code_registration_and_renamed_device_reconnect() -> Result<()> {
    let f = Fixture::new()?;
    f.register(false).await?;
    let device = Device::open(f.guest.root().into(), Some("Renamed guest".into()))?;
    let (server, _codes) = f.server();
    let (a, b) = tokio::io::duplex(3);
    let (host, guest) = tokio::join!(
        server.accept(Box::new(a)),
        connect(Box::new(b), &device, &f.endpoint)
    );
    let mut host = host?.context("missing Git session")?;
    let mut guest = guest?;
    let (sent, received) = tokio::join!(
        framing::send(&mut guest, b"binary\0\xff"),
        framing::receive(&mut host.stream)
    );
    sent?;
    assert_eq!(received?, b"binary\0\xff");
    assert_eq!(host.peer.name, "Renamed guest");
    Ok(())
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
#[tokio::test]
async fn revocation_refuses_new_connections_but_existing_stream_can_finish() -> Result<()> {
    let f = Fixture::new()?;
    f.register(false).await?;
    let (server, _) = f.server();
    let (a, b) = tokio::io::duplex(4096);
    let (host, guest) = tokio::join!(
        server.accept(Box::new(a)),
        connect(Box::new(b), &f.guest, &f.endpoint)
    );
    let mut host = host?.unwrap();
    let mut guest = guest?;
    f.share.revoke(&f.guest.peer().short_id())?;
    let (sent, received) = tokio::join!(
        framing::send(&mut guest, b"already authorized session"),
        framing::receive(&mut host.stream)
    );
    sent?;
    assert_eq!(received?, b"already authorized session");
    let (a, b) = tokio::io::duplex(4096);
    let (host, guest) = tokio::join!(
        server.accept(Box::new(a)),
        connect(Box::new(b), &f.guest, &f.endpoint)
    );
    assert!(host.is_err());
    assert!(guest.is_err());
    Ok(())
}
#[tokio::test]
async fn changed_host_key_is_never_automatically_trusted() -> Result<()> {
    let f = Fixture::new()?;
    f.register(false).await?;
    let other = Device::open(f._dir.path().join("other-host"), Some("Host".into()))?;
    let server = Server::new(other, f.share.clone(), Arc::new(|_| {}));
    let (a, b) = tokio::io::duplex(4096);
    let (host, guest) = tokio::join!(
        server.accept(Box::new(a)),
        connect(Box::new(b), &f.guest, &f.endpoint)
    );
    assert!(host.is_err());
    assert!(
        format!("{:#}", guest.err().context("changed host accepted")?).contains("host key changed")
    );
    Ok(())
}
#[tokio::test]
async fn claimed_registered_name_and_public_key_are_not_proof_of_possession() -> Result<()> {
    let f = Fixture::new()?;
    f.register(false).await?;
    let stranger = Device::open(f._dir.path().join("stranger"), Some("Guest".into()))?;
    let (server, _) = f.server();
    let (a, b) = tokio::io::duplex(4096);
    let attack = async {
        let mut b: Connection = Box::new(b);
        let request_ = Request {
            operation: Operation::Connect,
            share: Some(f.share.id()),
            peer: f.guest.peer().clone(),
        };
        request(&mut b, &request_).await?;
        let reply = reply(&mut b, Some(f.endpoint.share)).await?;
        let transcript = messages::transcript(&request_, &reply)?;
        handshake::exchange(b, &stranger, &reply.host, &transcript, None, true).await
    };
    let (host, guest) = tokio::join!(server.accept(Box::new(a)), attack);
    assert!(host.is_err());
    assert!(guest.is_err());
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

struct BufferedBridge {
    local: DuplexStream,
    relay: tokio::task::JoinHandle<()>,
}

impl BufferedBridge {
    fn wrap(remote: DuplexStream) -> Connection {
        let (local, bridge) = tokio::io::duplex(65536);
        let relay = tokio::spawn(async move {
            let (mut input, mut output) = tokio::io::split(bridge);
            let (mut incoming, mut outgoing) = tokio::io::split(remote);
            let send = async {
                let mut bytes = [0; 4096];
                loop {
                    let count = input.read(&mut bytes).await?;
                    if count == 0 {
                        return outgoing.shutdown().await;
                    }
                    tokio::time::sleep(Duration::from_millis(15)).await;
                    outgoing.write_all(&bytes[..count]).await?;
                    outgoing.flush().await?;
                }
            };
            let receive = async {
                tokio::io::copy(&mut incoming, &mut output).await?;
                output.shutdown().await
            };
            let _ = tokio::try_join!(send, receive);
        });
        Box::new(Self { local, relay })
    }
}

impl Drop for BufferedBridge {
    fn drop(&mut self) {
        self.relay.abort();
    }
}
impl AsyncRead for BufferedBridge {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.local).poll_read(cx, out)
    }
}
impl AsyncWrite for BufferedBridge {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.local).poll_write(cx, bytes)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.local).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.local).poll_shutdown(cx)
    }
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
    assert!(host?.is_none());
    assert_eq!(guest?.key, f.host.peer().key);
    assert!(f.share.member(&f.guest.peer().key)?.is_some());
    Ok(())
}
