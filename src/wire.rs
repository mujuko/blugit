use anyhow::{Context, Result, bail, ensure};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const CHUNK: usize = 32768;
pub const MAX_FRAME: usize = 65536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Request = 1,
    Ready = 2,
    Data = 3,
    Eof = 4,
    Stderr = 5,
    Exit = 6,
    Error = 7,
    Ack = 8,
}

pub async fn send<W: AsyncWrite + Unpin>(writer: &mut W, kind: Kind, data: &[u8]) -> Result<()> {
    ensure!(data.len() <= MAX_FRAME, "frame too large");
    let mut frame = Vec::with_capacity(5 + data.len());
    frame.push(kind as u8);
    frame.extend_from_slice(&(data.len() as u32).to_be_bytes());
    frame.extend_from_slice(data);
    writer.write_all(&frame).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn receive<R: AsyncRead + Unpin>(reader: &mut R) -> Result<(Kind, Vec<u8>)> {
    let kind = match reader.read_u8().await.context("read frame type")? {
        1 => Kind::Request,
        2 => Kind::Ready,
        3 => Kind::Data,
        4 => Kind::Eof,
        5 => Kind::Stderr,
        6 => Kind::Exit,
        7 => Kind::Error,
        8 => Kind::Ack,
        _ => bail!("unknown frame type"),
    };
    let size = reader
        .read_u32()
        .await
        .with_context(|| format!("read {kind:?} frame length"))? as usize;
    ensure!(size <= MAX_FRAME, "frame too large");
    let mut data = vec![0; size];
    reader
        .read_exact(&mut data)
        .await
        .with_context(|| format!("read {kind:?} payload ({size} bytes)"))?;
    Ok((kind, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn python_frame_and_oversized_payload() -> Result<()> {
        let mut bytes = &b"\x03\x00\x00\x00\x04\x00\xff\r\n"[..];
        let (kind, payload) = receive(&mut bytes).await?;
        assert_eq!(kind, Kind::Data);
        assert_eq!(payload, [0, 255, 13, 10]);
        assert!(receive(&mut &b"\x03\xff\xff\xff\xff"[..]).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn frames_survive_short_reads_and_writes() -> Result<()> {
        let (mut writer, mut reader) = tokio::io::duplex(2);
        let payload = [0, 255, 13, 10, 0, 1, 2];
        let sending = send(&mut writer, Kind::Data, &payload);
        let receiving = receive(&mut reader);
        let ((), (kind, received)) = tokio::try_join!(sending, receiving)?;
        assert_eq!(kind, Kind::Data);
        assert_eq!(received, payload);
        Ok(())
    }

    #[tokio::test]
    async fn truncated_frames_identify_the_failed_part() {
        for (bytes, expected) in [
            (&b""[..], "read frame type"),
            (&b"\x03\x00\x00"[..], "read Data frame length"),
            (&b"\x03\x00\x00\x00\x04x"[..], "read Data payload (4 bytes)"),
        ] {
            let error = receive(&mut &bytes[..])
                .await
                .expect_err("truncated frame must fail");
            assert!(format!("{error:#}").contains(expected), "{error:#}");
        }
    }
}
