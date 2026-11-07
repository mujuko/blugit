use super::{Code, framing, handshake};
use crate::{model::Peer, platform::Connection, storage::Device};
use anyhow::{Result, ensure};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use zeroize::Zeroizing;

pub(super) const CONFIRM: &[u8] = b"register this device";
pub(super) const ACCEPTED: &[u8] = b"registered";
pub(super) const ACK: &[u8] = b"registration saved";
pub(super) const DONE: &[u8] = b"registration complete";

pub(super) async fn exchange(
    mut stream: Connection,
    device: &Device,
    peer: &Peer,
    transcript: &[u8],
    code: Code,
    initiator: bool,
) -> Result<Connection> {
    let client_id = [b"blugit/v1/client/".as_slice(), transcript].concat();
    let host_id = [b"blugit/v1/host/".as_slice(), transcript].concat();
    let password = Password::new(code.as_str().as_bytes());
    let (pake, message) = if initiator {
        Spake2::<Ed25519Group>::start_a(
            &password,
            &Identity::new(&client_id),
            &Identity::new(&host_id),
        )
    } else {
        Spake2::<Ed25519Group>::start_b(
            &password,
            &Identity::new(&client_id),
            &Identity::new(&host_id),
        )
    };
    let incoming = if initiator {
        let incoming = framing::receive(&mut stream).await?;
        framing::send(&mut stream, &message).await?;
        incoming
    } else {
        framing::send(&mut stream, &message).await?;
        framing::receive(&mut stream).await?
    };
    ensure!(incoming.len() == 33, "invalid SPAKE2 message size");
    let secret = pake
        .finish(&incoming)
        .map_err(|_| anyhow::anyhow!("invalid SPAKE2 exchange"))?;
    let key = Zeroizing::new(
        <[u8; 32]>::try_from(secret).map_err(|_| anyhow::anyhow!("invalid SPAKE2 key length"))?,
    );
    handshake::exchange(stream, device, peer, transcript, Some(&key), initiator).await
}
