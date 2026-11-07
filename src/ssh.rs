//! SSH directly over the RFCOMM byte stream. Only a single Git exec is allowed.
use crate::{
    endpoint::Endpoint,
    git,
    model::Peer,
    platform::Connection,
    server::Shutdown,
    storage::{Device, ShareStore, TrustStore},
};
use anyhow::{Context, Result, bail, ensure};
use russh::{
    Channel, ChannelId, ChannelMsg,
    keys::{PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate},
};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context as TaskContext, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::{oneshot, watch},
};
use tokio_util::sync::{CancellationToken, DropGuard};

const SETUP_TIMEOUT: Duration = Duration::from_secs(15);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(test)]
mod tests;

/// Russh runs its protocol loop in a background task. Cancelling the owner must
/// also close that loop, the radio stream and any Git child owned by its handler.
struct CancelStream {
    inner: Connection,
    cancelled: Pin<Box<dyn Future<Output = ()> + Send>>,
}
impl CancelStream {
    fn wrap(inner: Connection) -> (Self, DropGuard) {
        let token = CancellationToken::new();
        let cancelled = Box::pin(token.clone().cancelled_owned());
        (Self { inner, cancelled }, token.drop_guard())
    }
    fn check(&mut self, cx: &mut TaskContext<'_>) -> std::io::Result<()> {
        if self.cancelled.as_mut().poll(cx).is_ready() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "SSH session cancelled",
            ));
        }
        Ok(())
    }
}
impl AsyncRead for CancelStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.check(cx)?;
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}
impl AsyncWrite for CancelStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.check(cx)?;
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        self.check(cx)?;
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

fn public_bytes(key: &PublicKey) -> Option<[u8; 32]> {
    key.key_data().ed25519().map(|key| key.0)
}

pub struct Client {
    host_key: [u8; 32],
}
impl russh::client::Handler for Client {
    type Error = anyhow::Error;
    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool> {
        ensure!(
            key.certificate().is_none() && public_bytes(&key.public_key()) == Some(self.host_key),
            "host key changed; verify the host in person and register again"
        );
        Ok(true)
    }
}

pub struct Session {
    handle: russh::client::Handle<Client>,
    share: crate::model::ShareId,
    _cancel: DropGuard,
}
impl Session {
    /// The virtual path is the registered share ID, never a filesystem path.
    pub async fn exec(&self, service: &str) -> Result<Channel<russh::client::Msg>> {
        ensure!(
            matches!(service, "git-upload-pack" | "git-receive-pack"),
            "unsupported Git service"
        );
        tokio::time::timeout(SETUP_TIMEOUT, async {
            let mut channel = self.handle.channel_open_session().await?;
            channel
                .exec(true, format!("{service} '{}'", self.share))
                .await?;
            let mut reason = Vec::new();
            loop {
                match channel.wait().await {
                    Some(ChannelMsg::Success) => return Ok(channel),
                    Some(ChannelMsg::ExtendedData { data, ext: 1 })
                        if reason.len() + data.len() <= 4096 =>
                    {
                        reason.extend_from_slice(&data)
                    }
                    _ => bail!(
                        "host refused the Git exec request: {}",
                        String::from_utf8_lossy(&reason)
                    ),
                }
            }
        })
        .await
        .context("SSH Git exec timed out")?
    }

    /// Confirm the close reply reached the host before dropping a buffered radio
    /// bridge. A standard SSH keepalive round trip replaces the custom ACK frames.
    pub async fn finish(self) -> Result<()> {
        tokio::time::timeout(CLOSE_TIMEOUT, async {
            // REQUEST_FAILURE is the normal reply to this OpenSSH keepalive.
            // Unlike send_ping(), this API reports a missing reply as an error.
            match self
                .handle
                .send_global_request("keepalive@openssh.com", b"", true)
                .await
            {
                Ok(_) | Err(russh::Error::RequestDenied) => {}
                Err(error) => return Err(error).context("confirm SSH channel close"),
            }
            self.handle
                .disconnect(
                    russh::Disconnect::ByApplication,
                    "Git transfer complete",
                    "en",
                )
                .await?;
            if let Err(error) = self.handle.await
                && !matches!(
                    error.downcast_ref::<russh::Error>(),
                    Some(russh::Error::Disconnect)
                )
            {
                return Err(error);
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("SSH completion timed out")?
    }
}

pub async fn connect(stream: Connection, device: &Device, endpoint: &Endpoint) -> Result<Session> {
    let host = TrustStore::new(device.root().into())
        .host(endpoint)?
        .context("repository not registered; run blugit join first")?;
    let host_key = host.ssh_key;
    let (stream, cancel) = CancelStream::wrap(stream);
    tokio::time::timeout(SETUP_TIMEOUT, async {
        let config = russh::client::Config { inactivity_timeout: None, ..Default::default() };
        let mut handle = russh::client::connect_stream(Arc::new(config), stream, Client { host_key }).await?;
        let auth = handle.authenticate_publickey(
            device.peer().id(),
            PrivateKeyWithHashAlg::new(Arc::new(device.ssh_private_key()), None),
        ).await?;
        ensure!(auth.success(), "SSH public key authentication refused; device is not registered or was revoked; run blugit join");
        Ok(Session { handle, share: endpoint.share, _cancel: cancel })
    }).await.context("SSH authentication timed out")?
}

struct Server {
    share: ShareStore,
    shutdown: watch::Receiver<Shutdown>,
    peer: Option<Peer>,
    channel: Option<Channel<russh::server::Msg>>,
    opened: bool,
    active: Arc<AtomicBool>,
    done: Option<oneshot::Sender<Result<()>>>,
    job: Option<tokio::task::JoinHandle<()>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.abort();
        }
    }
}
impl Server {
    fn member(&self, user: &str, key: &PublicKey) -> Result<Option<Peer>> {
        if *self.shutdown.borrow() != Shutdown::Running
            || user.len() != 64
            || !user.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Ok(None);
        }
        let mut id = [0; 32];
        hex::decode_to_slice(user, &mut id)?;
        Ok(self
            .share
            .member(&id)?
            .filter(|peer| Some(peer.ssh_key) == public_bytes(key)))
    }
}
impl russh::server::Handler for Server {
    type Error = anyhow::Error;
    async fn channel_close(&mut self, _: ChannelId, _: &mut russh::server::Session) -> Result<()> {
        // A peer that initiates close before our output completes cancels Git.
        // Do not wait for a relay that may be blocked on the peer's SSH window.
        ensure!(
            self.job.as_ref().is_none_or(|job| job.is_finished()),
            "guest closed SSH channel before Git completed"
        );
        Ok(())
    }
    async fn shell_request(
        &mut self,
        id: ChannelId,
        session: &mut russh::server::Session,
    ) -> Result<()> {
        session.channel_failure(id)?;
        Ok(())
    }
    async fn subsystem_request(
        &mut self,
        id: ChannelId,
        _: &str,
        session: &mut russh::server::Session,
    ) -> Result<()> {
        session.channel_failure(id)?;
        Ok(())
    }
    async fn env_request(
        &mut self,
        id: ChannelId,
        _: &str,
        _: &str,
        session: &mut russh::server::Session,
    ) -> Result<()> {
        session.channel_failure(id)?;
        Ok(())
    }
    async fn pty_request(
        &mut self,
        id: ChannelId,
        _: &str,
        _: u32,
        _: u32,
        _: u32,
        _: u32,
        _: &[(russh::Pty, u32)],
        session: &mut russh::server::Session,
    ) -> Result<()> {
        session.channel_failure(id)?;
        Ok(())
    }
    async fn auth_publickey_offered(
        &mut self,
        user: &str,
        offer: russh::server::PublicKeyOffer<'_>,
    ) -> Result<russh::server::Auth> {
        Ok(if self.member(user, offer.public_key)?.is_some() {
            russh::server::Auth::Accept
        } else {
            russh::server::Auth::reject()
        })
    }
    async fn auth_publickey(
        &mut self,
        user: &str,
        key: russh::server::VerifiedPublicKey<'_>,
    ) -> Result<russh::server::Auth> {
        self.peer = self.member(user, key.public_key)?;
        Ok(if self.peer.is_some() {
            russh::server::Auth::Accept
        } else {
            russh::server::Auth::reject()
        })
    }
    async fn channel_open_session(
        &mut self,
        channel: Channel<russh::server::Msg>,
        reply: russh::server::ChannelOpenHandle,
        _: &mut russh::server::Session,
    ) -> Result<()> {
        if !self.opened && self.peer.is_some() && *self.shutdown.borrow() == Shutdown::Running {
            self.opened = true;
            self.channel = Some(channel);
            reply.accept().await;
        }
        Ok(())
    }
    async fn exec_request(
        &mut self,
        id: ChannelId,
        command: &[u8],
        session: &mut russh::server::Session,
    ) -> Result<()> {
        let service = git::parse_exec(command, self.share.id());
        let authorized = match &self.peer {
            Some(peer) => self
                .share
                .member(&peer.key)?
                .is_some_and(|current| current.ssh_key == peer.ssh_key),
            None => false,
        };
        if service.is_none()
            || !authorized
            || *self.shutdown.borrow() != Shutdown::Running
            || self
                .channel
                .as_ref()
                .is_none_or(|channel| channel.id() != id)
        {
            let reason = if service.is_none() {
                "share ID does not match the hosted repository, or unsupported Git command"
            } else if !authorized {
                "device access was revoked"
            } else {
                "host is stopping, or a Git command was already started"
            };
            session.extended_data(id, 1, reason.as_bytes().to_vec())?;
            session.channel_failure(id)?;
            return Ok(());
        }
        let child = match git::spawn(service.unwrap(), self.share.repository()) {
            Ok(child) => child,
            Err(error) => {
                eprintln!("Cannot start Git: {error:#}");
                session.channel_failure(id)?;
                return Ok(());
            }
        };
        // No await between the stop/revocation checks and granting this operation.
        self.active.store(true, Ordering::Release);
        session.channel_success(id)?;
        let channel = self.channel.take().context("missing Git channel")?;
        let done = self.done.take().context("duplicate Git exec")?;
        let peer = self.peer.as_ref().unwrap();
        eprintln!(
            "Authenticated SSH device={:?} id={}",
            peer.name,
            peer.short_id()
        );
        self.job = Some(tokio::spawn(async move {
            let _ = done.send(git::serve(channel, child).await);
        }));
        Ok(())
    }
}

pub async fn serve(
    stream: Connection,
    device: &Device,
    share: ShareStore,
    mut shutdown: watch::Receiver<Shutdown>,
) -> Result<()> {
    let (stream, _cancel) = CancelStream::wrap(stream);
    let config = russh::server::Config {
        keys: vec![device.ssh_private_key()],
        methods: (&[russh::MethodKind::PublicKey][..]).into(),
        auth_rejection_time: Duration::from_millis(250),
        max_auth_attempts: 3,
        inactivity_timeout: None,
        ..Default::default()
    };
    let active = Arc::new(AtomicBool::new(false));
    let (done, mut completed) = oneshot::channel();
    let handler = Server {
        share,
        shutdown: shutdown.clone(),
        peer: None,
        channel: None,
        opened: false,
        active: active.clone(),
        done: Some(done),
        job: None,
    };
    let running = tokio::select! {
        _ = shutdown.changed() => return Ok(()),
        result = tokio::time::timeout(SETUP_TIMEOUT, russh::server::run_stream(Arc::new(config), stream, handler)) => result.context("SSH identification timed out")??,
    };
    tokio::pin!(running);
    let deadline = tokio::time::sleep(SETUP_TIMEOUT);
    tokio::pin!(deadline);
    let mut setup_pending = true;
    loop {
        tokio::select! {
            result = &mut running => {
                return match completed.try_recv() {
                    Ok(Ok(())) => after_git_close(result),
                    Ok(Err(error)) => Err(error),
                    Err(_) => result.context("SSH session"),
                };
            }
            result = &mut completed => {
                match result {
                    Ok(result) => result?,
                    Err(_) => return running.await.context("SSH Git task stopped"),
                }
                return after_git_close(tokio::time::timeout(CLOSE_TIMEOUT, &mut running).await.context("SSH disconnect timed out")?);
            }
            _ = &mut deadline, if setup_pending => {
                ensure!(active.load(Ordering::Acquire), "SSH authentication or Git exec timed out");
                setup_pending = false;
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() == Shutdown::Abort || !active.load(Ordering::Acquire) { return Ok(()); }
            }
        }
    }
}

fn after_git_close(result: Result<()>) -> Result<()> {
    match result {
        // Git has exited and queued its output, exit status, EOF and channel close.
        // A buffered bridge may drop the final SSH disconnect when it is released.
        Err(error)
            if matches!(
                error.downcast_ref::<russh::Error>(),
                Some(russh::Error::Disconnect | russh::Error::HUP)
            ) =>
        {
            Ok(())
        }
        Err(error)
            if matches!(error.downcast_ref::<russh::Error>(),
            Some(russh::Error::IO(io)) if matches!(io.kind(), std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe)) =>
        {
            Ok(())
        }
        result => result,
    }
}
