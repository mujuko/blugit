use super::Connection;
use crate::endpoint::{Address, Channel, Contact};
use anyhow::{Context, Result, ensure};
use bluer::rfcomm::{self, SocketAddr};
async fn adapter() -> Result<(bluer::Session, bluer::Adapter)> {
    let session = bluer::Session::new()
        .await
        .context("connect to BlueZ; is bluetoothd running?")?;
    let adapter = session.default_adapter().await?;
    ensure!(adapter.is_powered().await?, "Bluetooth is switched off");
    Ok((session, adapter))
}
pub async fn local_address() -> Result<Address> {
    let (_session, adapter) = adapter().await?;
    adapter.address().await?.to_string().parse()
}
pub async fn connect(e: &Contact) -> Result<Connection> {
    Ok(Box::new(
        rfcomm::Stream::connect(SocketAddr::new(
            e.address.to_string().parse()?,
            e.channel.get(),
        ))
        .await?,
    ))
}
pub struct Listener {
    pub address: Address,
    pub channel: u8,
    socket: rfcomm::Listener,
    _session: bluer::Session,
}
pub async fn listen(channel: Option<Channel>) -> Result<Listener> {
    let (session, adapter) = adapter().await?;
    let local = adapter.address().await?;
    // Channel zero requests atomic kernel allocation of an available channel.
    let socket = rfcomm::Listener::bind(SocketAddr::new(local, channel.map_or(0, Channel::get)))
        .await
        .context("bind RFCOMM channel; omit --channel for automatic allocation")?;
    let selected =
        super::allocated_channel(channel, u32::from(socket.as_ref().local_addr()?.channel))?;
    Ok(Listener {
        address: local.to_string().parse()?,
        channel: selected.get(),
        socket,
        _session: session,
    })
}
impl Listener {
    pub async fn accept(&mut self) -> Result<Connection> {
        Ok(Box::new(self.socket.accept().await?.0))
    }
}
