//! A bundled child owns IOBluetooth's main run loop and permission identity.
//! The application and Git protocol are Rust; native callbacks bridge local
//! Unix sockets to RFCOMM without opening any IP listener.
use super::Connection;
use crate::endpoint::{Address, Endpoint};
use anyhow::{Context, Result, ensure};
use std::{
    ffi::CString,
    hash::{Hash, Hasher},
    io,
    os::{
        fd::AsRawFd,
        unix::{ffi::OsStrExt, fs::PermissionsExt},
    },
    path::PathBuf,
    pin::Pin,
    task::{Context as TaskContext, Poll},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader, ReadBuf};

unsafe extern "C" {
    fn blugit_macos_main(argc: i32, argv: *const *const libc::c_char) -> i32;
    fn blugit_spawn(
        executable: *const libc::c_char,
        argv: *const *mut libc::c_char,
        output: i32,
        pid: *mut libc::pid_t,
    ) -> i32;
}

pub fn internal_helper() -> Option<u8> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first()?.as_bytes() != b"__bluetooth" {
        return None;
    }
    let strings: Vec<_> = args
        .iter()
        .skip(1)
        .map(|arg| CString::new(arg.as_bytes()))
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    let pointers: Vec<_> = strings.iter().map(|arg| arg.as_ptr()).collect();
    // SAFETY: immutable C strings remain live for the entire native run loop.
    Some(unsafe { blugit_macos_main(pointers.len() as i32, pointers.as_ptr()) } as u8)
}

fn bundled_helper() -> Result<PathBuf> {
    let current = std::env::current_exe()?;
    let sibling = current.with_file_name("blugit");
    let bytes = std::fs::read(if sibling.is_file() { sibling } else { current })?;
    let mut hash = std::hash::DefaultHasher::new();
    bytes.hash(&mut hash);
    let home = std::env::var_os("HOME").context("HOME is unset")?;
    let parent = PathBuf::from(home)
        .join("Library/Caches/blugit")
        .join(format!("native-{:016x}", hash.finish()));
    let bundle = parent.join("blugit.app");
    let executable = bundle.join("Contents/MacOS/blugit-bluetooth");
    if executable.is_file() {
        return Ok(executable);
    }
    std::fs::create_dir_all(&parent)?;
    let stage = tempfile::tempdir_in(&parent)?;
    let staged_bundle = stage.path().join("blugit.app");
    let staged_binary = staged_bundle.join("Contents/MacOS/blugit-bluetooth");
    std::fs::create_dir_all(staged_binary.parent().context("bundle parent")?)?;
    std::fs::write(&staged_binary, bytes)?;
    std::fs::set_permissions(&staged_binary, std::fs::Permissions::from_mode(0o755))?;
    std::fs::write(
        staged_bundle.join("Contents/Info.plist"),
        include_str!("../../native/Info.plist"),
    )?;
    if let Err(error) = std::fs::rename(&staged_bundle, &bundle)
        && !executable.is_file()
    {
        return Err(error.into());
    }
    Ok(executable)
}

struct Process {
    pid: libc::pid_t,
}
impl Drop for Process {
    fn drop(&mut self) {
        let pid = self.pid;
        // SAFETY: this pid belongs to our live child, retained until this guard
        // drops. Reap it on a separate thread rather than blocking Tokio.
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        std::thread::spawn(move || unsafe {
            libc::waitpid(pid, std::ptr::null_mut(), 0);
        });
    }
}

async fn start(args: &[String]) -> Result<(Process, BufReader<tokio::net::UnixStream>)> {
    let executable = bundled_helper()?;
    let path = CString::new(executable.as_os_str().as_bytes())?;
    let strings: Vec<_> = std::iter::once(path.clone())
        .chain(std::iter::once(CString::new("__bluetooth")?))
        .chain(
            args.iter()
                .map(|arg| CString::new(arg.as_str()))
                .collect::<std::result::Result<Vec<_>, _>>()?,
        )
        .collect();
    let mut pointers: Vec<_> = strings.iter().map(|arg| arg.as_ptr().cast_mut()).collect();
    pointers.push(std::ptr::null_mut());
    let (ours, theirs) = std::os::unix::net::UnixStream::pair()?;
    let mut pid = 0;
    // SAFETY: argv is null-terminated and every pointee survives posix_spawn.
    // The bridge duplicates the output fd and disclaims responsibility so macOS
    // attributes Bluetooth permission to the bundled helper, not the terminal.
    let error = unsafe {
        blugit_spawn(
            path.as_ptr(),
            pointers.as_ptr(),
            theirs.as_raw_fd(),
            &mut pid,
        )
    };
    ensure!(
        error == 0,
        "spawn Bluetooth helper: {}",
        io::Error::from_raw_os_error(error)
    );
    drop(theirs);
    ours.set_nonblocking(true)?;
    Ok((
        Process { pid },
        BufReader::new(tokio::net::UnixStream::from_std(ours)?),
    ))
}

async fn line(output: &mut BufReader<tokio::net::UnixStream>) -> Result<String> {
    let mut value = String::new();
    let length =
        tokio::time::timeout(Duration::from_secs(60), output.read_line(&mut value)).await??;
    ensure!(
        length > 0 && length < 4096,
        "Bluetooth helper exited; check Bluetooth permissions and the radio"
    );
    Ok(value.trim_end().into())
}

pub async fn local_address() -> Result<Address> {
    let (_process, mut output) = start(&["address".into()]).await?;
    line(&mut output).await?.parse()
}

pub async fn devices() -> Result<Vec<(Address, String)>> {
    let (_process, mut output) = start(&["devices".into()]).await?;
    let mut result = Vec::new();
    loop {
        let value = line(&mut output).await?;
        if value == "END" {
            break;
        }
        let (address, name) = value.split_once('\t').context("invalid device info")?;
        result.push((address.parse()?, name.into()));
    }
    Ok(result)
}

pub async fn nearby(duration: Duration) -> Result<Vec<(Address, String)>> {
    let (_process, mut output) = start(&["nearby".into(), duration.as_secs().to_string()]).await?;
    let mut result = Vec::new();
    loop {
        let value = line(&mut output).await?;
        if value == "END" {
            break;
        }
        let (address, name) = value.split_once('\t').context("invalid inquiry result")?;
        result.push((address.parse()?, name.into()));
    }
    Ok(result)
}

struct Bridge {
    socket: tokio::net::UnixStream,
    _control: Option<BufReader<tokio::net::UnixStream>>,
    _process: std::sync::Arc<Process>,
    _directory: std::sync::Arc<tempfile::TempDir>,
}
impl AsyncRead for Bridge {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_read(cx, buffer)
    }
}
impl AsyncWrite for Bridge {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.socket).poll_write(cx, data)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_shutdown(cx)
    }
}

pub struct Listener {
    pub address: Address,
    pub channel: u8,
    socket: tokio::net::UnixListener,
    process: std::sync::Arc<Process>,
    directory: std::sync::Arc<tempfile::TempDir>,
    _output: BufReader<tokio::net::UnixStream>,
}

pub async fn listen() -> Result<Listener> {
    let directory = std::sync::Arc::new(tempfile::tempdir()?);
    let path = directory.path().join("stream");
    let socket = tokio::net::UnixListener::bind(&path)?;
    let (process, mut output) = start(&[
        "host".into(),
        path.to_str().context("socket path is not UTF-8")?.into(),
    ])
    .await?;
    let value = line(&mut output).await?;
    let fields: Vec<_> = value.split('\t').collect();
    ensure!(
        fields.len() == 3 && fields[0] == "READY",
        "invalid Bluetooth helper response: {value}"
    );
    let address = fields[1].parse()?;
    let channel = fields[2].parse()?;
    Ok(Listener {
        address,
        channel,
        socket,
        process: std::sync::Arc::new(process),
        directory,
        _output: output,
    })
}

impl Listener {
    pub async fn accept(&mut self) -> Result<Connection> {
        let (socket, _) = self.socket.accept().await?;
        Ok(Box::new(Bridge {
            socket,
            _control: None,
            _process: self.process.clone(),
            _directory: self.directory.clone(),
        }))
    }
}

pub async fn connect(endpoint: &Endpoint) -> Result<Connection> {
    let directory = std::sync::Arc::new(tempfile::tempdir()?);
    let path = directory.path().join("stream");
    let socket = tokio::net::UnixListener::bind(&path)?;
    let (process, mut output) = start(&[
        "connect".into(),
        endpoint.address.to_string(),
        endpoint.channel.unwrap_or(0).to_string(),
        path.to_str().context("socket path is not UTF-8")?.into(),
    ])
    .await?;
    ensure!(
        line(&mut output).await? == "POWER",
        "Bluetooth initialization failed"
    );
    ensure!(
        tokio::time::timeout(Duration::from_secs(30), line(&mut output)).await?? == "READY",
        "Bluetooth connection failed"
    );
    let (stream, _) = socket.accept().await?;
    // The control socket also lets the native helper detect parent termination.
    Ok(Box::new(Bridge {
        socket: stream,
        _control: Some(output),
        _process: std::sync::Arc::new(process),
        _directory: directory,
    }))
}

#[cfg(all(test, feature = "test-transport"))]
mod tests {
    use std::{
        io::{self, Read, Write},
        net::Shutdown,
        os::{fd::AsRawFd, unix::net::UnixStream},
        time::Duration,
    };

    unsafe extern "C" {
        fn blugit_test_dispatch_read(fd: i32, buffer: *mut u8, size: usize) -> isize;
    }

    #[test]
    fn dispatch_io_nonblocking_duplicate_waits_for_delayed_data() -> io::Result<()> {
        let (reader, mut peer) = UnixStream::pair()?;
        peer.set_read_timeout(Some(Duration::from_secs(5)))?;
        let expected: Vec<u8> = (0..4096).map(|index| index as u8).collect();
        let payload = expected.clone();
        let writer = std::thread::spawn(move || -> io::Result<()> {
            let mut marker = [0];
            peer.read_exact(&mut marker)?;
            assert_eq!(marker, *b"!");
            // The bridge is idle after Dispatch I/O has set O_NONBLOCK. This
            // used to turn read()'s EAGAIN into an immediate Bluetooth close.
            std::thread::sleep(Duration::from_millis(150));
            for chunk in payload.chunks(257) {
                peer.write_all(chunk)?;
                std::thread::sleep(Duration::from_millis(2));
            }
            peer.shutdown(Shutdown::Write)
        });
        let mut received = vec![0; expected.len()];
        // SAFETY: the Unix stream and output buffer remain valid throughout the
        // synchronous native regression harness; no Bluetooth API is called.
        let count = unsafe {
            blugit_test_dispatch_read(reader.as_raw_fd(), received.as_mut_ptr(), received.len())
        };
        writer.join().expect("Unix socket test writer panicked")?;
        assert_eq!(
            count,
            expected.len() as isize,
            "native read: {}",
            io::Error::last_os_error()
        );
        assert_eq!(received, expected);
        Ok(())
    }
}
