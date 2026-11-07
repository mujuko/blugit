use crate::platform::Connection;
use std::{
    pin::Pin,
    task::{Context as TaskContext, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};
pub(crate) struct BufferedBridge {
    local: DuplexStream,
    relay: tokio::task::JoinHandle<()>,
}

impl BufferedBridge {
    pub(crate) fn wrap(remote: DuplexStream) -> Connection {
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
