use crate::platform::Connection;
use anyhow::{Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MAX_PACKET: usize = 4096;
pub(super) async fn send(stream: &mut Connection, bytes: &[u8]) -> Result<()> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_PACKET,
        "invalid authentication packet size"
    );
    stream.write_u16(bytes.len() as u16).await?;
    stream.write_all(bytes).await?;
    stream.flush().await?;
    Ok(())
}
pub(super) async fn receive(stream: &mut Connection) -> Result<Vec<u8>> {
    let count = stream.read_u16().await? as usize;
    ensure!(
        (1..=MAX_PACKET).contains(&count),
        "invalid authentication packet size"
    );
    let mut bytes = vec![0; count];
    stream.read_exact(&mut bytes).await?;
    Ok(bytes)
}
pub(super) async fn send_json(stream: &mut Connection, value: &impl Serialize) -> Result<()> {
    send(stream, &serde_json::to_vec(value)?).await
}
pub(super) async fn receive_json<T: DeserializeOwned>(stream: &mut Connection) -> Result<T> {
    Ok(serde_json::from_slice(&receive(stream).await?)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn partial_packets_and_oversized_packets() -> Result<()> {
        let (a, b) = tokio::io::duplex(1);
        let mut a: Connection = Box::new(a);
        let mut b: Connection = Box::new(b);
        let (sent, received) = tokio::join!(send(&mut a, b"\0binary\xff"), receive(&mut b));
        sent?;
        assert_eq!(received?, b"\0binary\xff");
        let (a, b) = tokio::io::duplex(16);
        let mut a: Connection = Box::new(a);
        let mut b: Connection = Box::new(b);
        a.write_u16(4097).await?;
        assert!(receive(&mut b).await.is_err());
        Ok(())
    }
}
