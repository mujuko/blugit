//! Winsock RFCOMM with automatic or explicit channel allocation.
use super::Connection;
use crate::endpoint::{Address, Channel, Contact};
use anyhow::{Context, Result, ensure};
use socket2::{Domain, Protocol, SockAddr, SockAddrStorage, Socket, Type};
use std::{
    io,
    mem::{size_of, zeroed},
    ptr,
    time::Duration,
};
use windows_sys::{
    Win32::{Devices::Bluetooth::*, Foundation::CloseHandle},
    core::GUID,
};
// ws2bth.h defines this macro as ULONG(-1); windows-sys omits the macro.
const BT_PORT_ANY: u32 = u32::MAX;
fn address(address: Address, channel: u32) -> SockAddr {
    let raw = SOCKADDR_BTH {
        addressFamily: AF_BTH,
        btAddr: address.as_u64(),
        serviceClassId: GUID::from_u128(0),
        port: channel,
    };
    // SAFETY: copy the Windows packed sockaddr into aligned socket storage.
    unsafe {
        let mut storage: SockAddrStorage = zeroed();
        ptr::copy_nonoverlapping(
            ptr::from_ref(&raw).cast::<u8>(),
            ptr::from_mut(&mut storage).cast::<u8>(),
            size_of::<SOCKADDR_BTH>(),
        );
        SockAddr::new(storage, size_of::<SOCKADDR_BTH>() as i32)
    }
}
fn socket() -> io::Result<Socket> {
    Socket::new(
        Domain::from(AF_BTH as i32),
        Type::STREAM,
        Some(Protocol::from(BTHPROTO_RFCOMM as i32)),
    )
}
fn asynchronous(socket: Socket) -> Result<Connection> {
    Ok(Box::new(super::windows_stream::BluetoothStream::new(
        socket,
    )?))
}
pub async fn connect(endpoint: &Contact) -> Result<Connection> {
    connect_with_timeout(endpoint, Duration::from_secs(25)).await
}
pub async fn connect_with_timeout(endpoint: &Contact, budget: Duration) -> Result<Connection> {
    let endpoint = *endpoint;
    let socket = tokio::task::spawn_blocking(move || -> Result<Socket> {
        let socket = socket()?;
        socket
            .connect_timeout(
                &address(endpoint.address, u32::from(endpoint.channel.get())),
                budget,
            )
            .context("Bluetooth connection failed; check the host address and channel")?;
        Ok(socket)
    })
    .await??;
    asynchronous(socket)
}
pub async fn local_address() -> Result<Address> {
    tokio::task::spawn_blocking(|| -> Result<Address> {
        // SAFETY: initialize every Win32 size field and close find/radio handles.
        unsafe {
            let params = BLUETOOTH_FIND_RADIO_PARAMS {
                dwSize: size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32,
            };
            let mut radio = ptr::null_mut();
            let find = BluetoothFindFirstRadio(&params, &mut radio);
            ensure!(
                !find.is_null(),
                "no usable Bluetooth radio: {}",
                io::Error::last_os_error()
            );
            let mut result = Err(anyhow::anyhow!("no usable Bluetooth address"));
            loop {
                let mut info: BLUETOOTH_RADIO_INFO = zeroed();
                info.dwSize = size_of::<BLUETOOTH_RADIO_INFO>() as u32;
                let status = BluetoothGetRadioInfo(radio, &mut info);
                CloseHandle(radio);
                if status == 0
                    && let Ok(address) = Address::from_u64(info.address.Anonymous.ullLong)
                {
                    result = Ok(address);
                    break;
                }
                if BluetoothFindNextRadio(find, &mut radio) == 0 {
                    break;
                }
            }
            BluetoothFindRadioClose(find);
            result
        }
    })
    .await?
}
pub struct Listener {
    pub address: Address,
    pub channel: u8,
    socket: Socket,
}
fn bound_channel(requested: Option<Channel>, bound: &SockAddr) -> Result<Channel> {
    ensure!(
        bound.len() as usize >= size_of::<SOCKADDR_BTH>(),
        "invalid Bluetooth socket address length"
    );
    // SAFETY: the length is checked; SOCKADDR_BTH is packed on Windows.
    let raw = unsafe { ptr::read_unaligned(bound.as_ptr().cast::<SOCKADDR_BTH>()) };
    let family = raw.addressFamily;
    ensure!(
        family == AF_BTH,
        "OS returned a non-Bluetooth socket address"
    );
    super::allocated_channel(requested, raw.port)
}
pub async fn listen(channel: Option<Channel>) -> Result<Listener> {
    let local = local_address().await?;
    let socket = socket()?;
    socket
        .bind(&address(
            local,
            channel.map_or(BT_PORT_ANY, |c| u32::from(c.get())),
        ))
        .with_context(|| match channel {
            Some(c) => {
                format!("bind RFCOMM channel {c}; omit --channel to allocate a free channel")
            }
            None => "allocate a free RFCOMM channel".into(),
        })?;
    let channel = bound_channel(
        channel,
        &socket
            .local_addr()
            .context("read allocated RFCOMM channel")?,
    )?;
    socket.listen(8)?;
    socket.set_nonblocking(true)?;
    Ok(Listener {
        address: local,
        channel: channel.get(),
        socket,
    })
}
impl Listener {
    pub async fn accept(&mut self) -> Result<Connection> {
        loop {
            match self.socket.accept() {
                Ok((socket, _)) => return asynchronous(socket),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(Duration::from_millis(25)).await
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packed_bluetooth_sockaddr_layout() {
        assert_eq!(std::mem::size_of::<super::SOCKADDR_BTH>(), 30);
        assert_eq!(std::mem::offset_of!(super::SOCKADDR_BTH, port), 26);
    }
    #[test]
    fn allocated_port_is_read_from_packed_sockaddr_and_explicit_mismatches_fail() -> Result<()> {
        let local = "00:A7:50:41:8D:47".parse()?;
        assert_eq!(bound_channel(None, &address(local, 12))?.get(), 12);
        assert!(bound_channel(Some(Channel::try_from(7)?), &address(local, 12)).is_err());
        assert!(bound_channel(None, &address(local, BT_PORT_ANY)).is_err());
        let ipv4 = SockAddr::from(std::net::SocketAddr::from(([127, 0, 0, 1], 7)));
        assert!(bound_channel(None, &ipv4).is_err());
        Ok(())
    }
}
