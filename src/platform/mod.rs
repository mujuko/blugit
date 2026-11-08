//! Only byte-stream transport. No identities, permissions, repository paths or Git.
use crate::endpoint::{Address, Channel, Contact};
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};
pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
pub type Connection = Box<dyn Stream>;
pub const CHUNK: usize = 32768;
pub const SERVICE: uuid::Uuid = uuid::Uuid::from_u128(0x86b63824106648f5a694fd5ba0f90ed5);
#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_discovery;
#[cfg(windows)]
mod windows_stream;
#[cfg(windows)]
use windows as native;
#[cfg(windows)]
pub use windows_discovery::internal_helper as internal_discovery_helper;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;
#[cfg(target_os = "macos")]
pub use macos::internal_helper;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as native;
pub use native::Listener;
#[async_trait]
pub trait Acceptor: Send {
    async fn accept(&mut self) -> Result<Connection>;
}
#[async_trait]
impl Acceptor for Listener {
    async fn accept(&mut self) -> Result<Connection> {
        native::Listener::accept(self).await
    }
}
pub async fn listen(channel: Option<Channel>) -> Result<Listener> {
    native::listen(channel).await
}
/// Validate the channel actually reserved by the OS before publishing its code.
fn allocated_channel(requested: Option<Channel>, selected: u32) -> Result<Channel> {
    let selected = Channel::try_from(
        u8::try_from(selected).context("OS returned an invalid RFCOMM channel")?,
    )?;
    ensure!(
        requested.is_none_or(|channel| channel == selected),
        "requested RFCOMM channel is unavailable; omit --channel to allocate a free channel"
    );
    Ok(selected)
}
pub async fn local_address() -> Result<Address> {
    native::local_address().await
}
pub(crate) async fn discovery_channels(address: Address) -> Vec<Channel> {
    #[cfg(feature = "test-transport")]
    if std::env::var_os("BLUGIT_TEST_ADDR").is_some()
        || std::env::var_os("BLUGIT_TEST_ROUTES").is_some()
    {
        return Vec::new();
    }
    #[cfg(windows)]
    {
        windows_discovery::candidates(address).await
    }
    #[cfg(not(windows))]
    {
        let _ = address;
        Vec::new()
    }
}
pub async fn connect(endpoint: &Contact) -> Result<Connection> {
    #[cfg(feature = "test-transport")]
    if let Some(routes) = std::env::var_os("BLUGIT_TEST_ROUTES") {
        let routes: std::collections::BTreeMap<u8, std::net::SocketAddr> =
            serde_json::from_str(&routes.to_string_lossy())?;
        let address = routes
            .get(&endpoint.channel.get())
            .context("no test listener on this channel")?;
        ensure!(
            address.ip().is_loopback(),
            "test transport must use loopback"
        );
        return Ok(Box::new(
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::net::TcpStream::connect(address),
            )
            .await??,
        ));
    }
    #[cfg(feature = "test-transport")]
    if let Some(address) = std::env::var_os("BLUGIT_TEST_ADDR") {
        let address: std::net::SocketAddr = address.to_string_lossy().parse()?;
        anyhow::ensure!(
            address.ip().is_loopback(),
            "test transport must use loopback"
        );
        return Ok(Box::new(
            tokio::time::timeout(
                std::time::Duration::from_secs(15),
                tokio::net::TcpStream::connect(address),
            )
            .await??,
        ));
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

/// Short, serial probes on an already-enrolled device. The Windows native
/// deadline also bounds its blocking connect worker, so probes cannot pile up.
pub(crate) async fn probe(endpoint: &Contact) -> Result<Connection> {
    let budget = std::time::Duration::from_secs(5);
    #[cfg(feature = "test-transport")]
    if std::env::var_os("BLUGIT_TEST_ROUTES").is_some()
        || std::env::var_os("BLUGIT_TEST_ADDR").is_some()
    {
        return tokio::time::timeout(budget, connect(endpoint)).await?;
    }
    #[cfg(windows)]
    {
        native::connect_with_timeout(endpoint, budget).await
    }
    #[cfg(not(windows))]
    {
        tokio::time::timeout(budget, native::connect(endpoint)).await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_allocation_accepts_any_valid_channel_but_explicit_selection_must_match()
    -> Result<()> {
        for selected in 1..=30 {
            assert_eq!(allocated_channel(None, selected)?.get(), selected as u8);
        }
        let requested = Channel::try_from(8)?;
        assert_eq!(allocated_channel(Some(requested), 8)?, requested);
        assert!(allocated_channel(Some(requested), 9).is_err());
        for invalid in [0, 31, 256, u32::MAX] {
            assert!(allocated_channel(None, invalid).is_err());
        }
        Ok(())
    }
}
