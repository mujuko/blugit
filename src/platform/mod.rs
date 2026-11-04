use crate::endpoint::Endpoint;
use anyhow::Result;
use tokio::io::{AsyncRead, AsyncWrite};

pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
pub type Connection = Box<dyn Stream>;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_stream;
#[cfg(windows)]
use windows as native;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as native;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;
#[cfg(target_os = "macos")]
pub use macos::internal_helper;

pub use native::{Listener, devices, listen, local_address};

pub async fn connect(endpoint: &Endpoint) -> Result<Connection> {
    #[cfg(feature = "test-transport")]
    if let Ok(address) = std::env::var("BLUGIT_TEST_ADDR") {
        let socket: std::net::SocketAddr = address.parse()?;
        anyhow::ensure!(
            socket.ip().is_loopback(),
            "test transport must use loopback"
        );
        return Ok(Box::new(tokio::net::TcpStream::connect(socket).await?));
    }
    #[cfg(target_os = "macos")]
    {
        native::connect(endpoint).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            native::connect(endpoint),
        )
        .await?
    }
}
