//! Adapt Winsock RFCOMM to Tokio without TCP-specific IOCP/AFD assumptions.
//! One bounded reader and one writer thread serve each Bluetooth connection.
use socket2::Socket;
use std::{
    future::Future,
    io::{self, Read, Write},
    net::Shutdown,
    os::windows::io::AsRawSocket,
    pin::Pin,
    sync::mpsc,
    task::{Context, Poll, ready},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::{mpsc as async_mpsc, oneshot},
};

type Completion = oneshot::Receiver<io::Result<usize>>;
struct WriteJob {
    bytes: Vec<u8>,
    complete: oneshot::Sender<io::Result<usize>>,
}

pub struct BluetoothStream {
    socket: Socket,
    input: async_mpsc::Receiver<io::Result<Vec<u8>>>,
    unread: Vec<u8>,
    offset: usize,
    eof: bool,
    output: mpsc::Sender<WriteJob>,
    pending: Option<Completion>,
}

impl BluetoothStream {
    pub fn new(socket: Socket) -> io::Result<Self> {
        socket.set_nonblocking(false)?;
        let mut reader = socket.try_clone()?;
        let mut writer = socket.try_clone()?;
        let diagnostics = std::env::var_os("BLUGIT_BLUETOOTH_DIAGNOSTICS").is_some();
        let socket_id = socket.as_raw_socket();
        if diagnostics {
            eprintln!(
                "blugit Bluetooth: Winsock stream opened pid={} socket={socket_id}",
                std::process::id()
            );
        }
        let (send, input) = async_mpsc::channel(2);
        std::thread::spawn(move || {
            let mut received = false;
            loop {
                let mut bytes = vec![0; crate::platform::CHUNK];
                let result = match reader.read(&mut bytes) {
                    Ok(count) => {
                        if diagnostics && (!received || count == 0) {
                            eprintln!(
                                "blugit Bluetooth: Winsock read socket={socket_id} bytes={count}"
                            );
                        }
                        received |= count > 0;
                        bytes.truncate(count);
                        Ok(bytes)
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        if diagnostics {
                            eprintln!("blugit Bluetooth: Winsock read socket={socket_id}: {error}");
                        }
                        Err(error)
                    }
                };
                let ended = match &result {
                    Ok(bytes) => bytes.is_empty(),
                    Err(_) => true,
                };
                if send.blocking_send(result).is_err() || ended {
                    break;
                }
            }
        });
        let (output, receive) = mpsc::channel::<WriteJob>();
        std::thread::spawn(move || {
            let mut sent = false;
            for job in receive {
                let result = writer.write_all(&job.bytes).map(|()| job.bytes.len());
                let failed = result.is_err();
                if diagnostics && (!sent || failed) {
                    eprintln!("blugit Bluetooth: Winsock write socket={socket_id}: {result:?}");
                }
                sent |= !failed;
                let _ = job.complete.send(result);
                if failed {
                    break;
                }
            }
        });
        Ok(Self {
            socket,
            input,
            unread: Vec::new(),
            offset: 0,
            eof: false,
            output,
            pending: None,
        })
    }

    fn poll_pending(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let result = match self.pending.as_mut() {
            Some(pending) => ready!(Pin::new(pending).poll(cx)).unwrap_or_else(|_| {
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Bluetooth writer stopped",
                ))
            }),
            None => Ok(0),
        };
        self.pending = None;
        Poll::Ready(result)
    }
}

impl AsyncRead for BluetoothStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if buffer.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if self.offset == self.unread.len() && !self.eof {
            match ready!(self.input.poll_recv(cx)) {
                Some(Ok(bytes)) if !bytes.is_empty() => {
                    self.unread = bytes;
                    self.offset = 0;
                }
                Some(Err(error)) => {
                    self.eof = true;
                    return Poll::Ready(Err(error));
                }
                _ => self.eof = true,
            }
        }
        let count = buffer.remaining().min(self.unread.len() - self.offset);
        buffer.put_slice(&self.unread[self.offset..self.offset + count]);
        self.offset += count;
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for BluetoothStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.pending.is_none() {
            if bytes.is_empty() {
                return Poll::Ready(Ok(0));
            }
            let count = bytes.len().min(crate::platform::CHUNK);
            let (complete, pending) = oneshot::channel();
            // At most one job is ever outstanding: subsequent polls wait for its
            // completion, so the unbounded std channel cannot grow without bound.
            if self
                .output
                .send(WriteJob {
                    bytes: bytes[..count].to_vec(),
                    complete,
                })
                .is_err()
            {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Bluetooth writer stopped",
                )));
            }
            self.pending = Some(pending);
        }
        self.poll_pending(cx)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.poll_pending(cx))?;
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.as_mut().poll_flush(cx))?;
        Poll::Ready(self.socket.shutdown(Shutdown::Both))
    }
}

impl Drop for BluetoothStream {
    fn drop(&mut self) {
        // This wakes both blocking socket workers, including on Ctrl+C and a
        // cancelled connection. Dropping input also wakes a full blocking_send.
        let result = self.socket.shutdown(Shutdown::Both);
        if std::env::var_os("BLUGIT_BLUETOOTH_DIAGNOSTICS").is_some() {
            eprintln!(
                "blugit Bluetooth: Winsock stream dropped socket={} shutdown={result:?}",
                self.socket.as_raw_socket()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn native_socket_adapter_handles_binary_backpressure_and_eof() -> io::Result<()> {
        // The same Winsock read/write adapter is exercised without requiring a
        // radio. The IP socket exists only in this test.
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let client = std::net::TcpStream::connect(listener.local_addr()?)?;
        let (mut peer, _) = listener.accept()?;
        let data: Vec<u8> = (0..262144).map(|index| index as u8).collect();
        let expected = data.clone();
        let worker = std::thread::spawn(move || -> io::Result<Vec<u8>> {
            let mut bytes = vec![0; expected.len()];
            peer.read_exact(&mut bytes)?;
            peer.write_all(&bytes)?;
            peer.shutdown(Shutdown::Both)?;
            Ok(bytes)
        });
        let stream = BluetoothStream::new(client.into())?;
        let (mut reader, mut writer) = tokio::io::split(stream);
        let sending = writer.write_all(&data);
        let receiving = async {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await?;
            Ok::<_, io::Error>(bytes)
        };
        let ((), received) = tokio::try_join!(sending, receiving)?;
        assert_eq!(received, data);
        assert_eq!(worker.join().expect("native socket worker panicked")?, data);
        Ok(())
    }
}
