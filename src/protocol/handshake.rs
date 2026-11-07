use super::{framing, record::SecureStream};
use crate::{model::Peer, platform::Connection, storage::Device};
use anyhow::{Result, ensure};

pub(super) async fn exchange(
    mut stream: Connection,
    device: &Device,
    peer: &Peer,
    transcript: &[u8],
    psk: Option<&[u8; 32]>,
    initiator: bool,
) -> Result<Connection> {
    let pattern = if psk.is_some() {
        "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s"
    } else {
        "Noise_KK_25519_ChaChaPoly_BLAKE2s"
    };
    let mut builder = snow::Builder::new(pattern.parse()?)
        .local_private_key(device.secret())?
        .prologue(transcript)?;
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
    let mut message = vec![0; 4096];
    let mut output = vec![0; 4096];
    while !noise.is_handshake_finished() {
        if noise.is_my_turn() {
            let n = noise.write_message(&[], &mut message)?;
            framing::send(&mut stream, &message[..n]).await?;
        } else {
            let incoming = framing::receive(&mut stream).await?;
            ensure!(
                noise.read_message(&incoming, &mut output)? == 0,
                "unexpected Noise payload"
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
