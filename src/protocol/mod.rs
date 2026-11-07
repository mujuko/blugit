//! Authentication is independent of the radio, CLI, Git and local member-management commands.
mod code;
mod enrollment;
mod framing;
mod gate;
mod handshake;
mod messages;
mod record;
#[cfg(test)]
mod tests;

use crate::{
    endpoint::{Contact, Endpoint},
    model::{Peer, ShareId},
    platform::Connection,
    storage::{Device, ShareStore, TrustStore},
};
use anyhow::{Context, Result, bail, ensure};
pub use code::Code;
use messages::{Access, MAGIC, Operation, Reply, Request, Response};
use rand::{RngCore, rngs::OsRng};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::timeout,
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
pub struct EnrollmentNotice {
    pub share: ShareId,
    pub peer: Peer,
    pub code: Code,
}
pub type Notify = Arc<dyn Fn(EnrollmentNotice) + Send + Sync>;
pub struct Session {
    pub peer: Peer,
    pub stream: Connection,
}
pub struct Server {
    device: Device,
    share: ShareStore,
    gate: gate::Gate,
    notify: Notify,
}

impl Server {
    pub fn new(device: Device, share: ShareStore, notify: Notify) -> Self {
        Self {
            device,
            share,
            gate: gate::Gate::default(),
            notify,
        }
    }
    pub async fn accept(&self, mut stream: Connection) -> Result<Option<Session>> {
        let request: Request = timeout(HANDSHAKE_TIMEOUT, async {
            let mut magic = vec![0; MAGIC.len()];
            stream.read_exact(&mut magic).await?;
            ensure!(magic == MAGIC, "expected authenticated blugit v1 protocol");
            framing::receive_json(&mut stream).await
        })
        .await
        .context("initial authentication request timed out")??;
        request.peer.validate()?;
        if request.share.is_some_and(|id| id != self.share.id()) {
            reject(&mut stream, "share ID does not match the hosted repository").await?;
            bail!("wrong repository requested");
        }
        if matches!(request.operation, Operation::Connect) && request.share.is_none() {
            reject(&mut stream, "Git connection requires a registered share ID").await?;
            bail!("missing repository ID");
        }
        let mut nonce = [0; 16];
        OsRng.fill_bytes(&mut nonce);
        let reply = Reply {
            share: self.share.id(),
            host: self.device.peer().clone(),
            nonce,
        };
        let transcript = messages::transcript(&request, &reply)?;
        match request.operation {
            Operation::Enroll => {
                let _permit = match self.gate.admit() {
                    Ok(p) => p,
                    Err(e) => {
                        reject(&mut stream, &e.to_string()).await?;
                        return Err(e);
                    }
                };
                timeout(self.gate.lifetime, async {
                    let code = Code::generate();
                    (self.notify)(EnrollmentNotice {
                        share: self.share.id(),
                        peer: request.peer.clone(),
                        code: code.clone(),
                    });
                    framing::send_json(&mut stream, &Response::Ready { reply }).await?;
                    let mut secure = enrollment::exchange(
                        stream,
                        &self.device,
                        &request.peer,
                        &transcript,
                        code,
                        false,
                    )
                    .await
                    .context("one-time code verification failed")?;
                    ensure!(
                        framing::receive(&mut secure).await? == enrollment::CONFIRM,
                        "missing registration confirmation"
                    );
                    self.share.grant(&request.peer)?;
                    framing::send(&mut secure, enrollment::ACCEPTED).await?;
                    ensure!(
                        framing::receive(&mut secure).await? == enrollment::ACK,
                        "device registered, but registration acknowledgement missing"
                    );
                    framing::send(&mut secure, enrollment::DONE).await?;
                    Ok(None)
                })
                .await
                .context("registration expired; request a new code")?
            }
            Operation::Connect => {
                if self.share.member(&request.peer.key)?.is_none() {
                    reject(
                        &mut stream,
                        "device is not registered for this repository; run blugit join",
                    )
                    .await?;
                    bail!("unregistered device");
                }
                timeout(HANDSHAKE_TIMEOUT, async {
                    framing::send_json(&mut stream, &Response::Ready { reply }).await?;
                    let mut secure = handshake::exchange(
                        stream,
                        &self.device,
                        &request.peer,
                        &transcript,
                        None,
                        false,
                    )
                    .await?;
                    // Revocation during the key exchange must not open a new Git session.
                    if self.share.member(&request.peer.key)?.is_none() {
                        framing::send_json(&mut secure, &Access::Denied).await?;
                        bail!("device was revoked during authentication");
                    }
                    framing::send_json(&mut secure, &Access::Granted).await?;
                    Ok(Some(Session {
                        peer: request.peer,
                        stream: secure,
                    }))
                })
                .await
                .context("device authentication timed out")?
            }
        }
    }
}

async fn request(stream: &mut Connection, request: &Request) -> Result<()> {
    stream.write_all(MAGIC).await?;
    framing::send_json(stream, request).await
}
async fn reply(stream: &mut Connection, expected_share: Option<ShareId>) -> Result<Reply> {
    match framing::receive_json(stream).await? {
        Response::Ready { reply } => {
            reply.host.validate()?;
            ensure!(
                expected_share.is_none_or(|id| reply.share == id),
                "host replied with a different repository ID"
            );
            Ok(reply)
        }
        Response::Rejected { reason } => bail!("host refused the request: {reason:?}"),
    }
}
async fn reject(stream: &mut Connection, reason: &str) -> Result<()> {
    timeout(
        HANDSHAKE_TIMEOUT,
        framing::send_json(
            stream,
            &Response::Rejected {
                reason: reason.into(),
            },
        ),
    )
    .await??;
    Ok(())
}

pub async fn enroll<F, Fut>(
    stream: Connection,
    device: &Device,
    endpoint: &Endpoint,
    get_code: F,
) -> Result<Peer>
where
    F: FnOnce(Peer) -> Fut,
    Fut: Future<Output = Result<Code>>,
{
    Ok(enroll_at(
        stream,
        device,
        endpoint.contact(),
        Some(endpoint.share),
        get_code,
    )
    .await?
    .1)
}

/// Discover the hosted repository through a code-authenticated exchange.
/// An existing local name supplies expected_share to preserve its repository binding.
pub async fn enroll_at<F, Fut>(
    mut stream: Connection,
    device: &Device,
    contact: Contact,
    expected_share: Option<ShareId>,
    get_code: F,
) -> Result<(Endpoint, Peer)>
where
    F: FnOnce(Peer) -> Fut,
    Fut: Future<Output = Result<Code>>,
{
    let request_ = Request {
        operation: Operation::Enroll,
        share: expected_share,
        peer: device.peer().clone(),
    };
    let reply = timeout(HANDSHAKE_TIMEOUT, async {
        request(&mut stream, &request_).await?;
        reply(&mut stream, expected_share).await
    })
    .await??;
    let transcript = messages::transcript(&request_, &reply)?;
    let endpoint = Endpoint {
        address: contact.address,
        channel: contact.channel,
        share: reply.share,
    };
    timeout(Duration::from_secs(180), async {
        let code = get_code(reply.host.clone()).await?;
        let mut secure = enrollment::exchange(stream, device, &reply.host, &transcript, code, true)
            .await
            .context("code verification failed; request a new code")?;
        framing::send(&mut secure, enrollment::CONFIRM).await?;
        ensure!(
            framing::receive(&mut secure).await? == enrollment::ACCEPTED,
            "registration was not accepted"
        );
        TrustStore::new(device.root().into()).pin(&endpoint, &reply.host)?;
        framing::send(&mut secure, enrollment::ACK).await?;
        ensure!(
            framing::receive(&mut secure).await? == enrollment::DONE,
            "registration saved locally, but completion was not acknowledged"
        );
        Ok((endpoint, reply.host))
    })
    .await
    .context("registration expired")?
}

pub async fn connect(
    mut stream: Connection,
    device: &Device,
    endpoint: &Endpoint,
) -> Result<Connection> {
    let host = TrustStore::new(device.root().into())
        .host(endpoint)?
        .context("repository not registered; run blugit join first")?;
    timeout(HANDSHAKE_TIMEOUT, async {
        let request_ = Request {
            operation: Operation::Connect,
            share: Some(endpoint.share),
            peer: device.peer().clone(),
        };
        request(&mut stream, &request_).await?;
        let reply = reply(&mut stream, Some(endpoint.share)).await?;
        ensure!(
            reply.host.key == host.key,
            "host key changed; verify the host in person and register again"
        );
        let transcript = messages::transcript(&request_, &reply)?;
        let mut secure =
            handshake::exchange(stream, device, &host, &transcript, None, true).await?;
        match framing::receive_json(&mut secure).await? {
            Access::Granted => Ok(secure),
            Access::Denied => bail!("access to this repository was revoked"),
        }
    })
    .await
    .context("device authentication timed out")?
}
