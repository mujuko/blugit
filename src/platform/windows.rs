//! RFCOMM through Winsock, without TCP-specific connect, accept or I/O APIs.
use super::Connection;
use crate::{
    SERVICE,
    endpoint::{Address, Endpoint},
};
use anyhow::{Context, Result, ensure};
use socket2::{Domain, Protocol, SockAddr, SockAddrStorage, Socket, Type};
use std::{
    io,
    mem::{size_of, zeroed},
    ptr,
    time::Duration,
};
use windows_sys::{
    Win32::{Devices::Bluetooth::*, Foundation::CloseHandle, Networking::WinSock::*},
    core::GUID,
};

const NS_BTH: u32 = 16;

fn service_guid() -> GUID {
    GUID::from_u128(SERVICE.as_u128())
}

fn bluetooth_address(address: Address, channel: Option<u8>) -> SockAddr {
    // Windows declares SOCKADDR_BTH packed, with port at byte 26 (30 bytes total).
    let native = SOCKADDR_BTH {
        addressFamily: AF_BTH,
        btAddr: address.as_u64(),
        serviceClassId: if channel.is_some() {
            GUID::from_u128(0)
        } else {
            service_guid()
        },
        port: channel.map(u32::from).unwrap_or(0),
    };
    // SAFETY: storage is large and aligned enough; only the initialized 30 bytes
    // of this Windows-defined sockaddr are exposed to socket2/Winsock.
    unsafe {
        let mut storage: SockAddrStorage = zeroed();
        ptr::copy_nonoverlapping(
            ptr::from_ref(&native).cast::<u8>(),
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

pub async fn connect(endpoint: &Endpoint) -> Result<Connection> {
    let endpoint = endpoint.clone();
    let socket = tokio::task::spawn_blocking(move || -> Result<Socket> {
        let socket = socket()?;
        let address = bluetooth_address(endpoint.address, endpoint.channel);
        // With port=0 Winsock resolves our service UUID using SDP itself.
        socket
            .connect_timeout(&address, Duration::from_secs(25))
            .context("Bluetooth connect failed; check the host and try its Direct channel URL")?;
        Ok(socket)
    })
    .await??;
    asynchronous(socket)
}

fn radio_address() -> Result<Address> {
    // SAFETY: all Win32 structures have their required size set and handles are
    // closed on both success and failure paths before returning.
    unsafe {
        let params = BLUETOOTH_FIND_RADIO_PARAMS {
            dwSize: size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32,
        };
        let mut radio = ptr::null_mut();
        let find = BluetoothFindFirstRadio(&params, &mut radio);
        ensure!(
            !find.is_null(),
            "No usable Bluetooth radio: {}",
            io::Error::last_os_error()
        );
        let mut result = Err(anyhow::anyhow!("No usable Bluetooth address"));
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
}

pub async fn local_address() -> Result<Address> {
    tokio::task::spawn_blocking(radio_address).await?
}

pub async fn devices() -> Result<Vec<(Address, String)>> {
    tokio::task::spawn_blocking(|| -> Result<_> {
        let mut result = Vec::new();
        // SAFETY: enumeration uses initialized Win32 structs, no live inquiry,
        // and a find handle whose lifetime ends before returning.
        unsafe {
            let mut params: BLUETOOTH_DEVICE_SEARCH_PARAMS = zeroed();
            params.dwSize = size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32;
            params.fReturnAuthenticated = 1;
            let mut info: BLUETOOTH_DEVICE_INFO = zeroed();
            info.dwSize = size_of::<BLUETOOTH_DEVICE_INFO>() as u32;
            let find = BluetoothFindFirstDevice(&params, &mut info);
            if find.is_null() {
                return Ok(result);
            }
            loop {
                if let Ok(address) = Address::from_u64(info.Address.Anonymous.ullLong) {
                    let length = info
                        .szName
                        .iter()
                        .position(|ch| *ch == 0)
                        .unwrap_or(info.szName.len());
                    result.push((address, String::from_utf16_lossy(&info.szName[..length])));
                }
                if BluetoothFindNextDevice(find, &mut info) == 0 {
                    break;
                }
            }
            BluetoothFindDeviceClose(find);
        }
        Ok(result)
    })
    .await?
}

fn publish(channel: u8, operation: WSAESETSERVICEOP) -> Result<()> {
    let mut identifier = service_guid();
    let mut address = SOCKADDR_BTH {
        addressFamily: AF_BTH,
        btAddr: 0,
        serviceClassId: identifier,
        port: u32::from(channel),
    };
    let mut where_ = CSADDR_INFO {
        LocalAddr: SOCKET_ADDRESS {
            lpSockaddr: ptr::from_mut(&mut address).cast(),
            iSockaddrLength: size_of::<SOCKADDR_BTH>() as i32,
        },
        RemoteAddr: SOCKET_ADDRESS {
            lpSockaddr: ptr::null_mut(),
            iSockaddrLength: 0,
        },
        iSocketType: SOCK_STREAM,
        iProtocol: BTHPROTO_RFCOMM as i32,
    };
    let mut name: Vec<u16> = "blugit prototype\0".encode_utf16().collect();
    let query = WSAQUERYSETW {
        dwSize: size_of::<WSAQUERYSETW>() as u32,
        lpszServiceInstanceName: name.as_mut_ptr(),
        lpServiceClassId: &mut identifier,
        dwNameSpace: NS_BTH,
        dwNumberOfCsAddrs: 1,
        lpcsaBuffer: &mut where_,
        ..Default::default()
    };
    // SAFETY: all pointers in the query remain alive for this synchronous call.
    if unsafe { WSASetServiceW(&query, operation, 0) } != 0 {
        return Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }).into());
    }
    Ok(())
}

pub struct Listener {
    pub address: Address,
    pub channel: u8,
    socket: Socket,
}

pub async fn listen() -> Result<Listener> {
    let address = local_address().await?;
    let socket = socket()?;
    // Bind explicitly to the same radio whose address we display.
    let native = SOCKADDR_BTH {
        addressFamily: AF_BTH,
        btAddr: address.as_u64(),
        serviceClassId: GUID::from_u128(0),
        port: u32::MAX,
    };
    let bind_address = unsafe {
        let mut storage: SockAddrStorage = zeroed();
        ptr::copy_nonoverlapping(
            ptr::from_ref(&native).cast::<u8>(),
            ptr::from_mut(&mut storage).cast::<u8>(),
            size_of::<SOCKADDR_BTH>(),
        );
        SockAddr::new(storage, size_of::<SOCKADDR_BTH>() as i32)
    };
    socket
        .bind(&bind_address)
        .context("bind Bluetooth RFCOMM socket")?;
    socket.listen(4)?;
    let actual = socket.local_addr()?;
    // SAFETY: local_addr belongs to the AF_BTH socket we just bound. Read packed
    // data by value, never create a reference to an unaligned field.
    let channel = unsafe { ptr::read_unaligned(actual.as_ptr().cast::<SOCKADDR_BTH>()).port };
    ensure!(
        (1..=30).contains(&channel),
        "invalid assigned RFCOMM channel {channel}"
    );
    publish(channel as u8, RNRSERVICE_REGISTER).context("publish Bluetooth SDP service")?;
    socket.set_nonblocking(true)?;
    Ok(Listener {
        address,
        channel: channel as u8,
        socket,
    })
}

impl Listener {
    pub async fn accept(&mut self) -> Result<Connection> {
        loop {
            match self.socket.accept() {
                Ok((socket, _)) => return asynchronous(socket),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(Duration::from_millis(25)).await
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = publish(self.channel, RNRSERVICE_DELETE);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn winsock_bluetooth_address_has_packed_layout() {
        assert_eq!(std::mem::size_of::<super::SOCKADDR_BTH>(), 30);
        assert_eq!(std::mem::offset_of!(super::SOCKADDR_BTH, port), 26);
    }
}
