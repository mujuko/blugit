//! SDP is an unauthenticated hint, queried only on the already-known device.
//! Isolate the blocking provider in a killable child rather than leaking workers.
use crate::endpoint::{Address, Channel};
use anyhow::{Context, Result, ensure};
use socket2::{Domain, Protocol, Socket, Type};
use std::{mem::size_of, process::Stdio, ptr, time::Duration};
use windows_sys::{
    Win32::{
        Devices::Bluetooth::{AF_BTH, BTHPROTO_RFCOMM, NS_BTH, SOCKADDR_BTH},
        Foundation::HANDLE,
        Networking::WinSock::{
            CSADDR_INFO, LUP_FLUSHCACHE, LUP_RETURN_ADDR, LUP_RETURN_BLOB, SOCKET_ERROR,
            WSA_E_NO_MORE, WSAAddressToStringW, WSAGetLastError, WSALookupServiceBeginW,
            WSALookupServiceEnd, WSALookupServiceNextW, WSAQUERYSETW, WSASERVICE_NOT_FOUND,
        },
        System::Com::BLOB,
    },
    core::GUID,
};

const MODE: &str = "__discover-bluetooth";
const MAX_RESPONSE: usize = 65536;

pub fn internal_helper() -> Option<u8> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1)?.to_str()? != MODE {
        return None;
    }
    if args.len() != 3 {
        return Some(2);
    }
    let result = (|| -> Result<()> {
        let address = args[2]
            .to_str()
            .context("invalid Bluetooth address")?
            .parse()?;
        let channels = query(address)?;
        serde_json::to_writer(std::io::stdout().lock(), &channels)?;
        Ok(())
    })();
    Some(match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("blugit: SDP discovery: {error:#}");
            1
        }
    })
}

pub(super) async fn candidates(address: Address) -> Vec<Channel> {
    let result = async {
        let mut command = tokio::process::Command::new(std::env::current_exe()?);
        command
            .arg(MODE)
            .arg(address.to_string())
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(8), command.output())
            .await
            .context("SDP discovery timed out")??;
        ensure!(
            output.status.success(),
            "SDP provider failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        ensure!(output.stdout.len() <= 512, "oversized SDP helper response");
        let channels: Vec<u8> = serde_json::from_slice(&output.stdout)?;
        ensure!(channels.len() <= 30, "too many SDP candidate channels");
        channels
            .into_iter()
            .map(Channel::try_from)
            .collect::<Result<Vec<_>>>()
    }
    .await;
    match result {
        Ok(channels) => channels,
        Err(error) => {
            if std::env::var_os("BLUGIT_BLUETOOTH_DIAGNOSTICS").is_some() {
                eprintln!("blugit: {error:#}");
            }
            Vec::new()
        }
    }
}

struct Lookup(HANDLE);
impl Drop for Lookup {
    fn drop(&mut self) {
        // SAFETY: the successful Begin call transferred this handle to us.
        unsafe {
            WSALookupServiceEnd(self.0);
        }
    }
}

fn last_error() -> std::io::Error {
    // SAFETY: reads only this thread's last Winsock error.
    std::io::Error::from_raw_os_error(unsafe { WSAGetLastError() })
}

fn query(address: Address) -> Result<Vec<u8>> {
    // Socket construction initializes Winsock. This socket is never connected.
    let _winsock = Socket::new(
        Domain::from(i32::from(AF_BTH)),
        Type::STREAM,
        Some(Protocol::from(BTHPROTO_RFCOMM as i32)),
    )?;
    let mut raw = SOCKADDR_BTH {
        addressFamily: AF_BTH,
        btAddr: address.as_u64(),
        serviceClassId: GUID::from_u128(0),
        port: 0,
    };
    let mut context = [0u16; 128];
    let mut length = context.len() as u32;
    // SAFETY: the initialized sockaddr and UTF-16 output buffer are live.
    if unsafe {
        WSAAddressToStringW(
            ptr::from_mut(&mut raw).cast(),
            size_of::<SOCKADDR_BTH>() as u32,
            ptr::null(),
            context.as_mut_ptr(),
            &mut length,
        )
    } == SOCKET_ERROR
    {
        return Err(last_error().into());
    }
    let mut service = GUID::from_u128(super::SERVICE.as_u128());
    let request = WSAQUERYSETW {
        dwSize: size_of::<WSAQUERYSETW>() as u32,
        lpServiceClassId: &mut service,
        dwNameSpace: NS_BTH,
        lpszContext: context.as_mut_ptr(),
        ..WSAQUERYSETW::default()
    };
    let mut handle = ptr::null_mut();
    // SAFETY: every referenced query field remains live until enumeration ends.
    if unsafe { WSALookupServiceBeginW(&request, LUP_FLUSHCACHE, &mut handle) } == SOCKET_ERROR {
        let error = last_error();
        if error.raw_os_error() == Some(WSASERVICE_NOT_FOUND) {
            return Ok(Vec::new());
        }
        return Err(error.into());
    }
    let lookup = Lookup(handle);
    let mut hints = Vec::new();
    for _ in 0..128 {
        // u64 storage provides sufficient alignment for WSAQUERYSETW and its
        // nested fields on supported Windows architectures.
        let mut storage = vec![0u64; MAX_RESPONSE / size_of::<u64>()];
        let response = storage.as_mut_ptr().cast::<WSAQUERYSETW>();
        let mut length = MAX_RESPONSE as u32;
        // SAFETY: aligned, zeroed storage is larger than the query header.
        unsafe {
            (*response).dwSize = size_of::<WSAQUERYSETW>() as u32;
        }
        // SAFETY: provider may write at most the declared allocation length.
        if unsafe {
            WSALookupServiceNextW(
                lookup.0,
                LUP_RETURN_ADDR | LUP_RETURN_BLOB,
                &mut length,
                response,
            )
        } == SOCKET_ERROR
        {
            let error = last_error();
            if matches!(
                error.raw_os_error(),
                Some(WSA_E_NO_MORE | WSASERVICE_NOT_FOUND)
            ) {
                break;
            }
            return Err(error.into());
        }
        // SAFETY: the provider initialized the response header on success.
        let response = unsafe { ptr::read(response) };
        // SAFETY: storage contains initialized bytes and stays live throughout parsing.
        let bytes =
            unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), MAX_RESPONSE) };
        let created = if response.lpBlob.is_null() {
            0
        } else {
            let header = within(bytes, response.lpBlob.cast(), size_of::<BLOB>())?;
            // SAFETY: range checked above; read_unaligned avoids trusting nested alignment.
            let blob = unsafe { ptr::read_unaligned(header.as_ptr().cast::<BLOB>()) };
            generation(within(bytes, blob.pBlobData, blob.cbSize as usize)?)
        };
        let count = usize::try_from(response.dwNumberOfCsAddrs)?;
        ensure!(count <= 128, "too many SDP addresses");
        if count == 0 {
            continue;
        }
        let addresses = within(
            bytes,
            response.lpcsaBuffer.cast(),
            count * size_of::<CSADDR_INFO>(),
        )?;
        for entry in addresses.as_chunks::<{ size_of::<CSADDR_INFO>() }>().0 {
            // SAFETY: exact, checked chunk of provider initialized storage.
            let entry = unsafe { ptr::read_unaligned(entry.as_ptr().cast::<CSADDR_INFO>()) };
            if entry.RemoteAddr.iSockaddrLength < size_of::<SOCKADDR_BTH>() as i32 {
                continue;
            }
            let peer = within(
                bytes,
                entry.RemoteAddr.lpSockaddr.cast(),
                size_of::<SOCKADDR_BTH>(),
            )?;
            // SAFETY: the packed Bluetooth sockaddr range was checked above.
            let peer = unsafe { ptr::read_unaligned(peer.as_ptr().cast::<SOCKADDR_BTH>()) };
            // An advertised address can never redirect discovery to another device.
            if peer.addressFamily != AF_BTH || peer.btAddr != address.as_u64() {
                continue;
            }
            if let Ok(channel) = u8::try_from(peer.port)
                && Channel::try_from(channel).is_ok()
            {
                hints.push((channel, created));
            }
        }
    }
    Ok(rank(hints))
}

fn within(bytes: &[u8], pointer: *const u8, length: usize) -> Result<&[u8]> {
    let start = (pointer as usize)
        .checked_sub(bytes.as_ptr() as usize)
        .context("invalid SDP result pointer")?;
    let end = start.checked_add(length).context("oversized SDP result")?;
    bytes
        .get(start..end)
        .context("SDP result points outside the response")
}

fn rank(hints: Vec<(u8, u64)>) -> Vec<u8> {
    let mut unique: Vec<(u8, u64)> = Vec::new();
    for (channel, created) in hints {
        if Channel::try_from(channel).is_err() {
            continue;
        }
        if let Some(previous) = unique.iter_mut().find(|entry| entry.0 == channel) {
            previous.1 = previous.1.max(created);
        } else {
            unique.push((channel, created));
        }
    }
    unique.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    unique.into_iter().map(|entry| entry.0).collect()
}

fn element(bytes: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&header, bytes) = bytes.split_first()?;
    if header == 0 {
        return Some((0, &[], bytes));
    }
    let size = header & 7;
    let (length, bytes) = if size <= 4 {
        (1usize << size, bytes)
    } else {
        let width = 1usize << (size - 5);
        let encoded = bytes.get(..width)?;
        let length = encoded.iter().try_fold(0usize, |n, b| {
            n.checked_mul(256)?.checked_add(usize::from(*b))
        })?;
        (length, bytes.get(width..)?)
    };
    Some((header >> 3, bytes.get(..length)?, bytes.get(length..)?))
}

fn generation(bytes: &[u8]) -> u64 {
    let Some((6, mut fields, [])) = element(bytes) else {
        return 0;
    };
    let mut found = None;
    while !fields.is_empty() {
        let Some((1, [hi, lo], rest)) = element(fields) else {
            return 0;
        };
        let attribute = u16::from_be_bytes([*hi, *lo]);
        let Some((kind, value, rest)) = element(rest) else {
            return 0;
        };
        if attribute == 0x0200 {
            if found.is_some() || kind != 6 {
                return 0;
            }
            // IOBluetooth's published uint64 swaps its 32-bit words on the
            // wire. A sequence of two network-order uint32s survives its IPC.
            let Some((1, [h0, h1, h2, h3], tail)) = element(value) else {
                return 0;
            };
            let Some((1, [l0, l1, l2, l3], [])) = element(tail) else {
                return 0;
            };
            let high = u32::from_be_bytes([*h0, *h1, *h2, *h3]);
            let low = u32::from_be_bytes([*l0, *l1, *l2, *l3]);
            found = Some((u64::from(high) << 32) | u64::from(low));
        }
        fields = rest;
    }
    found.unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_adverts_precede_stale_duplicates_and_invalid_channels() {
        assert_eq!(
            rank(vec![
                (8, 1),
                (4, 0),
                (10, 3),
                (8, 2),
                (10, 0),
                (31, u64::MAX)
            ]),
            [10, 8, 4]
        );
    }
    #[test]
    fn metadata_is_only_a_bounded_hint_and_malformed_records_are_ignored() {
        let timestamp = 1794134000123u64.to_be_bytes();
        let mut record = vec![0x35, 15, 0x09, 0x02, 0x00, 0x35, 10, 0x0a];
        record.extend_from_slice(&timestamp[..4]);
        record.push(0x0a);
        record.extend_from_slice(&timestamp[4..]);
        assert_eq!(generation(&record), 1794134000123);
        let mut extended = vec![0x35, 19, 0x09, 0x01, 0x01, 0];
        extended.extend_from_slice(&record[2..]);
        assert_eq!(generation(&extended), 1794134000123);
        for length in 0..record.len() {
            assert_eq!(generation(&record[..length]), 0);
        }
        let mut duplicate = vec![0x35, 30];
        duplicate.extend_from_slice(&record[2..]);
        duplicate.extend_from_slice(&record[2..]);
        assert_eq!(generation(&duplicate), 0);
        record[7] = 0x0b; // Wrong width for the high word.
        assert_eq!(generation(&record), 0);
        assert_eq!(generation(&[0x37, 255, 255, 255, 255]), 0);
    }
    #[test]
    fn publication_timestamp_matches_the_actual_macos_sdp_wire_record() {
        // Captured through Windows SDP from an owned, temporary macOS record.
        // 0x0200 is [uint32 high, uint32 low]; the following uint64 experiments
        // (0x0201/0x0202) demonstrate the SDK's word swap and are ignored.
        let wire = hex::decode(concat!(
            "35a10900000a4f4977c009000135111c35c576b39f1148ec9dc2e826e3572c19",
            "0900020a000000000900053503191002090006352409656e09006a0901000966",
            "7209006a09011009646509006a090120096a6109006a09013009000808ff0901",
            "002519626c756769742d7364702d656e636f64696e672d70726f626509020035",
            "0a0a000001a10abbe66c6d0902010bbbe66c6d000001a10902020bbbe66c6d000001a1"
        ))
        .unwrap();
        assert_eq!(generation(&wire), 1794153802861);
        // Do not rank the malformed uint64 form above current publications.
        assert_eq!(
            generation(&hex::decode("350c0902000bbbe66c6d000001a1").unwrap()),
            0
        );
    }
    #[test]
    fn pointers_outside_the_provider_buffer_are_rejected() {
        let bytes = [0u8; 16];
        assert!(within(&bytes, bytes.as_ptr(), 16).is_ok());
        assert!(within(&bytes, bytes.as_ptr(), 17).is_err());
        assert!(within(&bytes, ptr::null(), 1).is_err());
    }
}
