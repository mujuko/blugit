//! Bounded Noise records over any platform byte stream. No background relay task.
use crate::platform::Connection;
use snow::TransportState;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

const CHUNK: usize = 16384;
const MAX_RECORD: usize = CHUNK + 17;

pub struct SecureStream {
    socket: Connection,
    noise: TransportState,
    header: [u8; 2],
    header_used: usize,
    ciphertext: Vec<u8>,
    ciphertext_used: usize,
    plaintext: Vec<u8>,
    plaintext_used: usize,
    outgoing: Vec<u8>,
    outgoing_used: usize,
    read_closed: bool,
    write_closed: bool,
    failed: bool,
}

impl SecureStream {
    pub fn new(socket: Connection, noise: TransportState) -> Self {
        Self {
            socket,
            noise,
            header: [0; 2],
            header_used: 0,
            ciphertext: Vec::new(),
            ciphertext_used: 0,
            plaintext: Vec::new(),
            plaintext_used: 0,
            outgoing: Vec::new(),
            outgoing_used: 0,
            read_closed: false,
            write_closed: false,
            failed: false,
        }
    }

    fn queue(&mut self, payload: &[u8]) -> io::Result<()> {
        let mut encrypted = vec![0; payload.len() + 16];
        let count = self
            .noise
            .write_message(payload, &mut encrypted)
            .map_err(invalid)?;
        self.outgoing.clear();
        self.outgoing
            .extend_from_slice(&(count as u16).to_be_bytes());
        self.outgoing.extend_from_slice(&encrypted[..count]);
        self.outgoing_used = 0;
        Ok(())
    }

    fn drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.failed {
            return Poll::Ready(Err(invalid(
                "encrypted stream already failed authentication",
            )));
        }
        while self.outgoing_used < self.outgoing.len() {
            let count = ready!(
                Pin::new(&mut self.socket).poll_write(cx, &self.outgoing[self.outgoing_used..])
            )?;
            if count == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.outgoing_used += count;
        }
        self.outgoing.clear();
        self.outgoing_used = 0;
        Poll::Ready(Ok(()))
    }
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn fill(
    socket: &mut Connection,
    cx: &mut Context<'_>,
    bytes: &mut [u8],
    used: &mut usize,
) -> Poll<io::Result<()>> {
    while *used < bytes.len() {
        let mut buffer = ReadBuf::new(&mut bytes[*used..]);
        ready!(Pin::new(&mut *socket).poll_read(cx, &mut buffer))?;
        let count = buffer.filled().len();
        if count == 0 {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "encrypted stream truncated before authenticated close",
            )));
        }
        *used += count;
    }
    Poll::Ready(Ok(()))
}

impl AsyncRead for SecureStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.failed {
            return Poll::Ready(Err(invalid(
                "encrypted stream already failed authentication",
            )));
        }
        if buffer.remaining() == 0 || this.read_closed {
            return Poll::Ready(Ok(()));
        }
        if this.plaintext_used == this.plaintext.len() {
            ready!(fill(
                &mut this.socket,
                cx,
                &mut this.header,
                &mut this.header_used
            ))?;
            if this.ciphertext.is_empty() {
                let length = u16::from_be_bytes(this.header) as usize;
                if !(17..=MAX_RECORD).contains(&length) {
                    this.failed = true;
                    return Poll::Ready(Err(invalid("invalid encrypted record size")));
                }
                this.ciphertext.resize(length, 0);
            }
            ready!(fill(
                &mut this.socket,
                cx,
                &mut this.ciphertext,
                &mut this.ciphertext_used
            ))?;
            // Never expose the decrypt buffer until the tag has been verified,
            // including if a caller retries a read after an authentication error.
            let mut plaintext = vec![0; this.ciphertext.len()];
            let count = match this.noise.read_message(&this.ciphertext, &mut plaintext) {
                Ok(count) => count,
                Err(error) => {
                    this.failed = true;
                    return Poll::Ready(Err(invalid(error)));
                }
            };
            plaintext.truncate(count);
            this.header_used = 0;
            this.ciphertext.clear();
            this.ciphertext_used = 0;
            match plaintext.first() {
                Some(0) if count > 1 => this.plaintext_used = 1,
                Some(1) if count == 1 => {
                    this.read_closed = true;
                    return Poll::Ready(Ok(()));
                }
                _ => {
                    this.failed = true;
                    return Poll::Ready(Err(invalid("invalid encrypted record type")));
                }
            }
            this.plaintext = plaintext;
        }
        let count = buffer
            .remaining()
            .min(this.plaintext.len() - this.plaintext_used);
        buffer.put_slice(&this.plaintext[this.plaintext_used..this.plaintext_used + count]);
        this.plaintext_used += count;
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for SecureStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.write_closed {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        ready!(this.drain(cx))?;
        if data.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let count = data.len().min(CHUNK);
        let mut plaintext = Vec::with_capacity(count + 1);
        plaintext.push(0);
        plaintext.extend_from_slice(&data[..count]);
        this.queue(&plaintext)?;
        Poll::Ready(Ok(count))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.drain(cx))?;
        Pin::new(&mut this.socket).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.drain(cx))?;
        if !this.write_closed {
            this.queue(&[1])?;
            this.write_closed = true;
        }
        ready!(this.drain(cx))?;
        ready!(Pin::new(&mut this.socket).poll_flush(cx))?;
        Pin::new(&mut this.socket).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn states() -> (TransportState, TransportState) {
        let mut a = snow::Builder::new("Noise_NN_25519_ChaChaPoly_BLAKE2s".parse().unwrap())
            .build_initiator()
            .unwrap();
        let mut b = snow::Builder::new("Noise_NN_25519_ChaChaPoly_BLAKE2s".parse().unwrap())
            .build_responder()
            .unwrap();
        let mut message = [0; 256];
        let mut output = [0; 256];
        let count = a.write_message(&[], &mut message).unwrap();
        b.read_message(&message[..count], &mut output).unwrap();
        let count = b.write_message(&[], &mut message).unwrap();
        a.read_message(&message[..count], &mut output).unwrap();
        (
            a.into_transport_mode().unwrap(),
            b.into_transport_mode().unwrap(),
        )
    }

    #[tokio::test]
    async fn encrypted_stream_preserves_binary_backpressure_and_authenticated_eof() -> io::Result<()>
    {
        let (a, b) = states();
        let (socket_a, socket_b) = tokio::io::duplex(3);
        let a = SecureStream::new(Box::new(socket_a), a);
        let b = SecureStream::new(Box::new(socket_b), b);
        let (mut ar, mut aw) = tokio::io::split(a);
        let (mut br, mut bw) = tokio::io::split(b);
        let payload: Vec<u8> = (0..131072).map(|i| (i % 256) as u8).collect();
        let sending = async {
            aw.write_all(&payload).await?;
            aw.shutdown().await
        };
        let reflecting = async {
            let mut block = [0; 713];
            loop {
                let count = br.read(&mut block).await?;
                if count == 0 {
                    break;
                }
                bw.write_all(&block[..count]).await?;
                bw.flush().await?;
            }
            bw.shutdown().await
        };
        let receiving = async {
            let mut bytes = Vec::new();
            ar.read_to_end(&mut bytes).await?;
            Ok::<_, io::Error>(bytes)
        };
        let ((), (), received) = tokio::try_join!(sending, reflecting, receiving)?;
        assert_eq!(received, payload);
        Ok(())
    }

    #[tokio::test]
    async fn tampering_replay_and_truncation_are_rejected() -> io::Result<()> {
        for mode in ["tamper", "replay", "truncated", "missing-close"] {
            let (mut a, b) = states();
            let mut encrypted = [0; 128];
            let count = a.write_message(b"\0secret", &mut encrypted).unwrap();
            let mut bytes = (count as u16).to_be_bytes().to_vec();
            bytes.extend_from_slice(&encrypted[..count]);
            match mode {
                "tamper" => {
                    *bytes.last_mut().unwrap() ^= 1;
                }
                "replay" => {
                    let copy = bytes.clone();
                    bytes.extend_from_slice(&copy);
                }
                "truncated" => {
                    bytes.pop();
                }
                _ => {}
            }
            let (mut sender, receiver) = tokio::io::duplex(4096);
            sender.write_all(&bytes).await?;
            sender.shutdown().await?;
            let mut receiver = SecureStream::new(Box::new(receiver), b);
            let mut output = Vec::new();
            let error = receiver.read_to_end(&mut output).await.unwrap_err();
            assert_eq!(
                error.kind(),
                if matches!(mode, "tamper" | "replay") {
                    io::ErrorKind::InvalidData
                } else {
                    io::ErrorKind::UnexpectedEof
                }
            );
            assert!(mode != "tamper" || output.is_empty());
            if matches!(mode, "tamper" | "replay") {
                let mut retry = [0; 64];
                assert_eq!(
                    receiver.read(&mut retry).await.unwrap_err().kind(),
                    io::ErrorKind::InvalidData
                );
                assert_eq!(retry, [0; 64]);
            }
        }
        Ok(())
    }
}
