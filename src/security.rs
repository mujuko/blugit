//! Technical spike: one code/attempt, SPAKE2 bootstrap, pinned Noise reconnects.
use crate::{
    endpoint::Address,
    identity::{Peer, State},
    platform::Connection,
    secure_stream::SecureStream,
};
use anyhow::{Context, Result, bail, ensure};
use rand::{Rng, RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Semaphore,
    time::timeout,
};

const MAGIC: &[u8] = b"BLUGIT-SPIKE-1\n";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_HANDSHAKE: usize = 4096;
const CONFIRM: &[u8] = b"register this device";
const ACCEPTED: &[u8] = b"registered";
const REGISTRATION_ACK: &[u8] = b"registration saved";
const REGISTRATION_DONE: &[u8] = b"registration complete";
const AUTHENTICATED: &[u8] = b"authenticated";
pub const TEAM: &str = "prototype";

#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
enum Request {
    Discover,
    Enroll { peer: Peer },
    Connect { peer: Peer },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    peer: Peer,
    nonce: [u8; 16],
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", deny_unknown_fields)]
enum ReplyEnvelope {
    Ready { reply: Reply },
    Rejected { message: String },
}

async fn send_reply(stream: &mut Connection, reply: &Reply) -> Result<()> {
    send_json(
        stream,
        &ReplyEnvelope::Ready {
            reply: reply.clone(),
        },
    )
    .await
}

async fn receive_reply(stream: &mut Connection) -> Result<Reply> {
    match receive_json(stream).await? {
        ReplyEnvelope::Ready { reply } => {
            reply.peer.validate()?;
            Ok(reply)
        }
        ReplyEnvelope::Rejected { message } => bail!("host rejected request: {:?}", message),
    }
}

async fn reject(stream: &mut Connection, message: &str) -> Result<()> {
    send_json(
        stream,
        &ReplyEnvelope::Rejected {
            message: message.into(),
        },
    )
    .await
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advertisement {
    pub team: Option<String>,
    pub host: Option<Peer>,
}

pub struct EnrollmentNotice {
    pub peer: Peer,
    pub code: String,
}
pub type Notify = Arc<dyn Fn(EnrollmentNotice) + Send + Sync>;

pub struct EnrollmentGate {
    slot: Semaphore,
    last: Mutex<Option<Instant>>,
    cooldown: Duration,
    lifetime: Duration,
}

impl Default for EnrollmentGate {
    fn default() -> Self {
        Self {
            slot: Semaphore::new(1),
            last: Mutex::new(None),
            cooldown: Duration::from_secs(10),
            lifetime: Duration::from_secs(180),
        }
    }
}

impl EnrollmentGate {
    fn admit(&self) -> Result<tokio::sync::SemaphorePermit<'_>> {
        let permit = self
            .slot
            .try_acquire()
            .context("another enrollment is pending")?;
        let mut last = self
            .last
            .lock()
            .map_err(|_| anyhow::anyhow!("enrollment gate poisoned"))?;
        ensure!(
            last.is_none_or(|last| last.elapsed() >= self.cooldown),
            "enrollment rate limit; wait 10 seconds before requesting another code"
        );
        *last = Some(Instant::now());
        Ok(permit)
    }
}

pub async fn send_packet(stream: &mut Connection, bytes: &[u8]) -> Result<()> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_HANDSHAKE,
        "invalid handshake packet length"
    );
    stream.write_u16(bytes.len() as u16).await?;
    stream.write_all(bytes).await?;
    stream.flush().await?;
    Ok(())
}

pub async fn receive_packet(stream: &mut Connection) -> Result<Vec<u8>> {
    let length = stream.read_u16().await? as usize;
    ensure!(
        (1..=MAX_HANDSHAKE).contains(&length),
        "invalid handshake packet length"
    );
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await?;
    Ok(bytes)
}

async fn send_json(stream: &mut Connection, value: &impl Serialize) -> Result<()> {
    send_packet(stream, &serde_json::to_vec(value)?).await
}

async fn receive_json<T: DeserializeOwned>(stream: &mut Connection) -> Result<T> {
    Ok(serde_json::from_slice(&receive_packet(stream).await?)?)
}

async fn request(stream: &mut Connection, request: &Request) -> Result<()> {
    stream.write_all(MAGIC).await?;
    send_json(stream, request).await
}

pub async fn discover(mut stream: Connection) -> Result<Advertisement> {
    timeout(CONNECT_TIMEOUT, async {
        request(&mut stream, &Request::Discover).await?;
        let advertisement: Advertisement = receive_json(&mut stream).await?;
        if let Some(host) = &advertisement.host {
            host.validate()?;
        }
        Ok(advertisement)
    })
    .await
    .context("discovery probe timed out")?
}

pub async fn enroll<F, Fut>(
    mut stream: Connection,
    state: &State,
    address: Address,
    code: F,
) -> Result<Peer>
where
    F: FnOnce(Peer) -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let request_ = Request::Enroll {
        peer: state.peer().clone(),
    };
    let reply: Reply = timeout(CONNECT_TIMEOUT, async {
        request(&mut stream, &request_).await?;
        receive_reply(&mut stream).await
    })
    .await
    .context("host did not accept enrollment request")??;
    reply.peer.validate()?;
    let prologue = serde_json::to_vec(&(&request_, &reply))?;
    timeout(Duration::from_secs(180), async {
        let input = code(reply.peer.clone()).await?;
        ensure!(
            input.len() == 6 && input.bytes().all(|b| b.is_ascii_digit()),
            "enter exactly six digits; request a new code if this attempt fails"
        );
        let (pake, outgoing) = Spake2::<Ed25519Group>::start_a(
            &Password::new(input.as_bytes()),
            &Identity::new(&[b"blugit-spike/client/", &prologue[..]].concat()),
            &Identity::new(&[b"blugit-spike/host/", &prologue[..]].concat()),
        );
        let incoming = receive_packet(&mut stream).await?;
        send_packet(&mut stream, &outgoing).await?;
        let key: [u8; 32] = pake
            .finish(&incoming)
            .map_err(|_| anyhow::anyhow!("invalid PAKE exchange"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid PAKE key length"))?;
        let mut secure = handshake(stream, state, &reply.peer, &prologue, Some(&key), true)
            .await
            .context("code verification failed; request a new one-time code")?;
        send_packet(&mut secure, CONFIRM).await?;
        ensure!(
            receive_packet(&mut secure).await? == ACCEPTED,
            "enrollment was not accepted"
        );
        state.save_host(address, &reply.peer)?;
        // A local socket flush does not mean the separate macOS helper has sent
        // the bytes over RFCOMM. Wait for a reply to our final outgoing packet
        // before dropping the stream (which terminates that helper).
        send_packet(&mut secure, REGISTRATION_ACK)
            .await
            .context("host registered locally; send registration acknowledgement")?;
        ensure!(
            receive_packet(&mut secure)
                .await
                .context("host registered locally; await registration completion")?
                == REGISTRATION_DONE,
            "host registered locally, but invalid completion acknowledgement"
        );
        Ok(reply.peer)
    })
    .await
    .context("one-time code expired")?
}

pub async fn connect(
    mut stream: Connection,
    state: &State,
    address: Address,
) -> Result<Connection> {
    let host = state
        .host(address)?
        .context("host not registered; run 'blugit spike join URL' first")?;
    timeout(CONNECT_TIMEOUT, async {
        let request_ = Request::Connect {
            peer: state.peer().clone(),
        };
        request(&mut stream, &request_).await?;
        let reply = receive_reply(&mut stream).await?;
        ensure!(
            reply.peer.key == host.key,
            "host key changed; verify the host and enroll again"
        );
        let prologue = serde_json::to_vec(&(&request_, &reply))?;
        let mut secure = handshake(stream, state, &host, &prologue, None, true).await?;
        ensure!(
            receive_packet(&mut secure).await? == AUTHENTICATED,
            "host did not authenticate this device"
        );
        Ok(secure)
    })
    .await
    .context("device authentication timed out")?
}

/// Returns a Git-ready stream only after pinned, mutual key authentication.
pub async fn accept(
    mut stream: Connection,
    state: &State,
    hidden: bool,
    gate: &EnrollmentGate,
    notify: &Notify,
) -> Result<Option<Connection>> {
    let request_: Request = timeout(CONNECT_TIMEOUT, async {
        let mut magic = vec![0; MAGIC.len()];
        stream.read_exact(&mut magic).await?;
        ensure!(
            magic == MAGIC,
            "expected security-spike protocol; plaintext Git is not accepted"
        );
        receive_json(&mut stream).await
    })
    .await
    .context("initial spike request timed out")??;
    if matches!(request_, Request::Discover) {
        send_json(
            &mut stream,
            &Advertisement {
                team: (!hidden).then(|| TEAM.into()),
                host: (!hidden).then(|| state.peer().clone()),
            },
        )
        .await?;
        return Ok(None);
    }
    let peer = match &request_ {
        Request::Enroll { peer } | Request::Connect { peer } => peer,
        Request::Discover => unreachable!(),
    };
    peer.validate()?;
    let mut nonce = [0; 16];
    OsRng.fill_bytes(&mut nonce);
    let reply = Reply {
        peer: state.peer().clone(),
        nonce,
    };
    let prologue = serde_json::to_vec(&(&request_, &reply))?;
    match &request_ {
        Request::Enroll { .. } => {
            let _permit = match gate.admit() {
                Ok(permit) => permit,
                Err(error) => {
                    reject(&mut stream, &error.to_string()).await?;
                    return Err(error);
                }
            };
            timeout(gate.lifetime, async {
                let code = format!("{:06}", OsRng.gen_range(0..1_000_000u32));
                notify(EnrollmentNotice {
                    peer: peer.clone(),
                    code: code.clone(),
                });
                send_reply(&mut stream, &reply).await?;
                let (pake, outgoing) = Spake2::<Ed25519Group>::start_b(
                    &Password::new(code.as_bytes()),
                    &Identity::new(&[b"blugit-spike/client/", &prologue[..]].concat()),
                    &Identity::new(&[b"blugit-spike/host/", &prologue[..]].concat()),
                );
                send_packet(&mut stream, &outgoing).await?;
                let incoming = receive_packet(&mut stream).await?;
                let key: [u8; 32] = pake
                    .finish(&incoming)
                    .map_err(|_| anyhow::anyhow!("invalid PAKE exchange"))?
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("invalid PAKE key length"))?;
                let mut secure = handshake(stream, state, peer, &prologue, Some(&key), false)
                    .await
                    .context("one-time code verification failed")?;
                ensure!(
                    receive_packet(&mut secure).await? == CONFIRM,
                    "missing enrollment confirmation"
                );
                state.save_member(peer)?;
                send_packet(&mut secure, ACCEPTED).await?;
                ensure!(
                    receive_packet(&mut secure)
                        .await
                        .context("device registered; await guest acknowledgement")?
                        == REGISTRATION_ACK,
                    "device registered, but invalid guest acknowledgement"
                );
                send_packet(&mut secure, REGISTRATION_DONE).await?;
                eprintln!(
                    "Registered team={} device={:?} id={}",
                    TEAM,
                    peer.name,
                    peer.short_id()
                );
                Ok(None)
            })
            .await
            .context("enrollment expired; code invalidated")?
        }
        Request::Connect { .. } => {
            let Some(member) = state.member(&peer.key)? else {
                reject(
                    &mut stream,
                    "device is not a registered member; run blugit spike join",
                )
                .await?;
                bail!("device is not a registered member");
            };
            ensure!(member.key == peer.key, "member key mismatch");
            timeout(CONNECT_TIMEOUT, async {
                send_reply(&mut stream, &reply).await?;
                let mut secure = handshake(stream, state, peer, &prologue, None, false).await?;
                send_packet(&mut secure, AUTHENTICATED).await?;
                eprintln!(
                    "Authenticated device={:?} id={}",
                    peer.name,
                    peer.short_id()
                );
                Ok(Some(secure))
            })
            .await
            .context("device authentication timed out")?
        }
        Request::Discover => bail!("unexpected discovery"),
    }
}

async fn handshake(
    mut stream: Connection,
    state: &State,
    peer: &Peer,
    prologue: &[u8],
    psk: Option<&[u8; 32]>,
    initiator: bool,
) -> Result<Connection> {
    let pattern = if psk.is_some() {
        "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s"
    } else {
        "Noise_KK_25519_ChaChaPoly_BLAKE2s"
    };
    let mut builder = snow::Builder::new(pattern.parse()?)
        .local_private_key(state.private())?
        .prologue(prologue)?;
    if let Some(psk) = psk {
        builder = builder.psk(3, psk)?;
    } else {
        builder = builder.remote_public_key(&peer.key)?;
    }
    let mut noise = if initiator {
        builder.build_initiator()?
    } else {
        builder.build_responder()?
    };
    let mut outgoing = vec![0; MAX_HANDSHAKE];
    let mut payload = vec![0; MAX_HANDSHAKE];
    while !noise.is_handshake_finished() {
        if noise.is_my_turn() {
            let count = noise.write_message(&[], &mut outgoing)?;
            send_packet(&mut stream, &outgoing[..count]).await?;
        } else {
            let incoming = receive_packet(&mut stream).await?;
            ensure!(
                noise.read_message(&incoming, &mut payload)? == 0,
                "unexpected handshake payload"
            );
        }
    }
    ensure!(
        noise.get_remote_static() == Some(&peer.key[..]),
        "authenticated device key mismatch"
    );
    Ok(Box::new(SecureStream::new(
        stream,
        noise.into_transport_mode()?,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        pin::Pin,
        task::{Context as TaskContext, Poll},
    };
    use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};

    // Model macOS: writes/flushes reach a private local socket immediately,
    // but a different task sends them over the real transport later. Dropping
    // the wrapper kills that task and discards any still-buffered final bytes.
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
        fn poll_flush(
            mut self: Pin<&mut Self>,
            cx: &mut TaskContext<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.local).poll_flush(cx)
        }
        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut TaskContext<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.local).poll_shutdown(cx)
        }
    }

    fn state(name: &str) -> (tempfile::TempDir, State) {
        let directory = tempfile::tempdir().unwrap();
        let state = State::open(directory.path().into(), Some(name.into())).unwrap();
        (directory, state)
    }

    fn notice_channel() -> (
        Notify,
        tokio::sync::mpsc::UnboundedReceiver<EnrollmentNotice>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (
            Arc::new(move |notice| {
                let _ = tx.send(notice);
            }),
            rx,
        )
    }

    async fn registration(
        host: &State,
        guest: &State,
        wrong: bool,
    ) -> (Result<Option<Connection>>, Result<Peer>) {
        let (server, client) = tokio::io::duplex(67);
        let (notify, mut rx) = notice_channel();
        let gate = EnrollmentGate::default();
        timeout(Duration::from_secs(5), async {
            tokio::join!(
                accept(Box::new(server), host, false, &gate, &notify),
                enroll(
                    Box::new(client),
                    guest,
                    "AA:BB:CC:DD:EE:FF".parse().unwrap(),
                    |_| async move {
                        let notice = rx.recv().await.unwrap();
                        Ok(if wrong {
                            format!(
                                "{:06}",
                                (notice.code.parse::<u32>().unwrap() + 1) % 1_000_000
                            )
                        } else {
                            notice.code
                        })
                    }
                )
            )
        })
        .await
        .expect("enrollment deadlocked")
    }

    #[tokio::test]
    async fn code_enrollment_persists_keys_and_reconnects_after_rename_and_restart() -> Result<()> {
        let (host_dir, host) = state("Host");
        let (guest_dir, guest) = state("Guest");
        let (server, client) = registration(&host, &guest, false).await;
        assert!(server?.is_none());
        assert_eq!(client?.key, host.peer().key);
        let host = State::open(host_dir.path().into(), None)?;
        let guest = State::open(guest_dir.path().into(), Some("Renamed Guest".into()))?;
        assert!(host.member(&guest.peer().key)?.is_some());
        let (server, client) = tokio::io::duplex(3);
        let gate = EnrollmentGate::default();
        let (notify, _rx) = notice_channel();
        let address = "AA:BB:CC:DD:EE:FF".parse()?;
        let (server, client) = timeout(Duration::from_secs(5), async {
            tokio::join!(
                accept(Box::new(server), &host, false, &gate, &notify),
                connect(Box::new(client), &guest, address)
            )
        })
        .await?;
        let mut server = server?.unwrap();
        let mut client = client?;
        let (sent, received) = tokio::join!(
            send_packet(&mut client, b"authenticated binary\0\xff"),
            receive_packet(&mut server)
        );
        sent?;
        assert_eq!(received?, b"authenticated binary\0\xff");
        Ok(())
    }

    #[tokio::test]
    async fn wrong_code_never_registers_either_side() -> Result<()> {
        let (_host_dir, host) = state("Host");
        let (_guest_dir, guest) = state("Guest");
        let (server, client) = registration(&host, &guest, true).await;
        assert!(server.is_err());
        assert!(client.is_err());
        assert!(host.member(&guest.peer().key)?.is_none());
        assert!(guest.host("AA:BB:CC:DD:EE:FF".parse()?)?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn enrollment_waits_for_host_before_dropping_a_buffered_bridge() -> Result<()> {
        let (_host_dir, host) = state("Host");
        let (_guest_dir, guest) = state("Guest");
        let (server, client) = tokio::io::duplex(4096);
        let (notify, mut rx) = notice_channel();
        let gate = EnrollmentGate::default();
        let address = "AA:BB:CC:DD:EE:FF".parse()?;
        let (server, client) = timeout(Duration::from_secs(5), async {
            tokio::join!(
                accept(Box::new(server), &host, false, &gate, &notify),
                enroll(
                    BufferedBridge::wrap(client),
                    &guest,
                    address,
                    |_| async move { Ok(rx.recv().await.unwrap().code) }
                )
            )
        })
        .await?;
        assert!(
            server?.is_none(),
            "host must finish without a truncated-stream error"
        );
        assert_eq!(client?.key, host.peer().key);
        assert_eq!(
            host.member(&guest.peer().key)?.unwrap().key,
            guest.peer().key
        );
        assert_eq!(guest.host(address)?.unwrap().key, host.peer().key);
        Ok(())
    }

    #[tokio::test]
    async fn unknown_member_and_changed_host_key_are_rejected() -> Result<()> {
        let (_host_dir, host) = state("Host");
        let (_guest_dir, guest) = state("Same device name");
        let (_stranger_dir, stranger) = state("Same device name");
        let address: Address = "AA:BB:CC:DD:EE:FF".parse()?;
        guest.save_host(address, host.peer())?;
        let gate = EnrollmentGate::default();
        let (notify, _rx) = notice_channel();
        let (a, b) = tokio::io::duplex(4096);
        let (server, client) = tokio::join!(
            accept(Box::new(a), &host, false, &gate, &notify),
            connect(Box::new(b), &guest, address)
        );
        assert!(format!("{:#}", server.err().unwrap()).contains("not a registered member"));
        assert!(client.is_err());
        // Matching display names do not give an unregistered key any access.
        assert_ne!(guest.peer().key, stranger.peer().key);
        host.save_member(guest.peer())?;
        guest.save_host(address, stranger.peer())?;
        let (a, b) = tokio::io::duplex(4096);
        let (server, client) = tokio::join!(
            accept(Box::new(a), &host, false, &gate, &notify),
            connect(Box::new(b), &guest, address)
        );
        assert!(server.is_err());
        assert!(format!("{:#}", client.err().unwrap()).contains("host key changed"));
        Ok(())
    }

    #[tokio::test]
    async fn hidden_team_is_not_advertised_but_accepts_direct_enrollment() -> Result<()> {
        let (_host_dir, host) = state("Host");
        let (_guest_dir, guest) = state("Guest");
        for hidden in [false, true] {
            let gate = EnrollmentGate::default();
            let (notify, _rx) = notice_channel();
            let (a, b) = tokio::io::duplex(67);
            let (server, client) = tokio::join!(
                accept(Box::new(a), &host, hidden, &gate, &notify),
                discover(Box::new(b))
            );
            assert!(server?.is_none());
            let advertisement = client?;
            assert_eq!(advertisement.team.is_some(), !hidden);
            assert_eq!(advertisement.host.is_some(), !hidden);
        }
        let gate = EnrollmentGate::default();
        let (notify, mut rx) = notice_channel();
        let (a, b) = tokio::io::duplex(67);
        let (server, client) = tokio::join!(
            accept(Box::new(a), &host, true, &gate, &notify),
            enroll(
                Box::new(b),
                &guest,
                "AA:BB:CC:DD:EE:FF".parse()?,
                |_| async move { Ok(rx.recv().await.unwrap().code) }
            )
        );
        assert!(server?.is_none());
        assert_eq!(client?.key, host.peer().key);
        Ok(())
    }

    #[tokio::test]
    async fn expired_or_cancelled_request_never_registers_and_gate_limits_new_requests()
    -> Result<()> {
        let (_host_dir, host) = state("Host");
        let (_guest_dir, guest) = state("Guest");
        let gate = EnrollmentGate {
            lifetime: Duration::from_millis(30),
            ..Default::default()
        };
        let (notify, mut rx) = notice_channel();
        let (a, b) = tokio::io::duplex(4096);
        let pending = async {
            let mut b: Connection = Box::new(b);
            request(
                &mut b,
                &Request::Enroll {
                    peer: guest.peer().clone(),
                },
            )
            .await?;
            let _reply = receive_reply(&mut b).await?;
            let _code = rx.recv().await.unwrap();
            tokio::time::sleep(Duration::from_millis(80)).await;
            Ok::<(), anyhow::Error>(())
        };
        let (server, client) =
            tokio::join!(accept(Box::new(a), &host, false, &gate, &notify), pending);
        client?;
        assert!(format!("{:#}", server.err().unwrap()).contains("expired"));
        assert!(host.member(&guest.peer().key)?.is_none());
        assert!(gate.admit().is_err());
        // Cancelling before key confirmation must not add a member either.
        let gate = EnrollmentGate::default();
        let (notify, _rx) = notice_channel();
        let (a, b) = tokio::io::duplex(4096);
        let cancel = async {
            let mut b: Connection = Box::new(b);
            request(
                &mut b,
                &Request::Enroll {
                    peer: guest.peer().clone(),
                },
            )
            .await?;
            let _reply = receive_reply(&mut b).await?;
            Ok::<(), anyhow::Error>(())
        };
        let (server, client) =
            tokio::join!(accept(Box::new(a), &host, false, &gate, &notify), cancel);
        client?;
        assert!(server.is_err());
        assert!(host.member(&guest.peer().key)?.is_none());
        let gate = EnrollmentGate::default();
        let permit = gate.admit()?;
        assert!(gate.admit().is_err());
        drop(permit);
        assert!(gate.admit().is_err());
        Ok(())
    }

    #[tokio::test]
    async fn plaintext_git_and_oversized_handshake_are_rejected() -> Result<()> {
        let (_host_dir, host) = state("Host");
        let gate = EnrollmentGate::default();
        let (notify, _rx) = notice_channel();
        for bytes in [
            b"not the security protocol".to_vec(),
            [MAGIC, &5000u16.to_be_bytes()].concat(),
        ] {
            let (a, mut b) = tokio::io::duplex(4096);
            b.write_all(&bytes).await?;
            b.shutdown().await?;
            assert!(
                accept(Box::new(a), &host, false, &gate, &notify)
                    .await
                    .is_err()
            );
        }
        Ok(())
    }
}
