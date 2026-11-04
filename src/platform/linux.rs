use super::Connection;
use crate::{
    SERVICE,
    endpoint::{Address, Endpoint},
};
use anyhow::{Context, Result, ensure};
use bluer::rfcomm::{self, Profile, ProfileHandle, Role, SocketAddr};
use futures_util::StreamExt;

async fn adapter() -> Result<(bluer::Session, bluer::Adapter)> {
    let session = bluer::Session::new()
        .await
        .context("connect to BlueZ (is bluetoothd running?)")?;
    let adapter = session.default_adapter().await?;
    ensure!(adapter.is_powered().await?, "Bluetooth is switched off");
    Ok((session, adapter))
}

pub async fn local_address() -> Result<Address> {
    let (_session, adapter) = adapter().await?;
    adapter.address().await?.to_string().parse()
}

pub async fn devices() -> Result<Vec<(Address, String)>> {
    let (_session, adapter) = adapter().await?;
    let mut result = Vec::new();
    for address in adapter.device_addresses().await? {
        let device = adapter.device(address)?;
        if device.is_paired().await? {
            result.push((
                address.to_string().parse()?,
                device.name().await?.unwrap_or_else(|| "(unnamed)".into()),
            ));
        }
    }
    Ok(result)
}

unsafe extern "C" {
    fn blugit_sdp_channel(address: *const u8, uuid: *const u8) -> i32;
}

pub async fn connect(endpoint: &Endpoint) -> Result<Connection> {
    let channel = if let Some(channel) = endpoint.channel {
        channel
    } else {
        let address = endpoint.address.0;
        let channel = tokio::task::spawn_blocking(move || {
            // SAFETY: both fixed-size byte arrays are live throughout the native
            // query. The helper owns and releases all BlueZ SDP allocations.
            unsafe { blugit_sdp_channel(address.as_ptr(), SERVICE.as_bytes().as_ptr()) }
        })
        .await?;
        ensure!(
            (1..=30).contains(&channel),
            "Bluetooth SDP lookup failed; try the host's Direct channel URL (error {channel})"
        );
        channel as u8
    };
    let address = endpoint.address.to_string().parse()?;
    Ok(Box::new(
        rfcomm::Stream::connect(SocketAddr::new(address, channel)).await?,
    ))
}

pub struct Listener {
    pub address: Address,
    pub channel: u8,
    profile: ProfileHandle,
    _session: bluer::Session,
}

pub async fn listen() -> Result<Listener> {
    let (session, adapter) = adapter().await?;
    let address = adapter.address().await?;
    // Reserve a free channel briefly, then give that channel to BlueZ's profile
    // listener. ProfileHandle owns both advertising and incoming connections.
    let probe = rfcomm::Listener::bind(SocketAddr::new(address, 0)).await?;
    let channel = probe.as_ref().local_addr()?.channel;
    drop(probe);
    let profile = session
        .register_profile(Profile {
            uuid: SERVICE,
            name: Some("blugit prototype".into()),
            role: Some(Role::Server),
            channel: Some(u16::from(channel)),
            require_authentication: Some(false),
            require_authorization: Some(false),
            ..Default::default()
        })
        .await
        .context("register BlueZ RFCOMM profile")?;
    Ok(Listener {
        address: address.to_string().parse()?,
        channel,
        profile,
        _session: session,
    })
}

impl Listener {
    pub async fn accept(&mut self) -> Result<Connection> {
        let request = self.profile.next().await.context("BlueZ profile stopped")?;
        Ok(Box::new(request.accept()?))
    }
}
