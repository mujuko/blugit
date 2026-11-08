//! A bundled child owns IOBluetooth's main run loop and permission identity.
//! The application and Git protocol are Rust; native callbacks bridge local
//! Unix sockets to RFCOMM without opening any IP listener.
use super::Connection;
use crate::endpoint::{Address, Channel, Contact};
use anyhow::{Context, Result, bail, ensure};
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
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, BufReader, ReadBuf};

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
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/blugit-bluetooth"));
    let plist = include_str!("../../native/Info.plist");
    let mut hash = std::hash::DefaultHasher::new();
    bytes.hash(&mut hash);
    plist.hash(&mut hash);
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
    std::fs::write(staged_bundle.join("Contents/Info.plist"), plist)?;
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
    let value = value.trim_end();
    if let Some(error) = value.strip_prefix("ERROR\t") {
        bail!("Bluetooth helper: {error}");
    }
    Ok(value.into())
}

pub async fn local_address() -> Result<Address> {
    let (_process, mut output) = start(&["address".into()]).await?;
    line(&mut output).await?.parse()
}

const DISCOVERY_OUTPUT_LIMIT: usize = 128;

fn parse_discovery_channels(bytes: &[u8]) -> Result<Vec<Channel>> {
    ensure!(
        bytes.len() <= DISCOVERY_OUTPUT_LIMIT,
        "SDP helper output is too large"
    );
    let line = std::str::from_utf8(bytes)?
        .strip_suffix('\n')
        .context("incomplete SDP helper response")?;
    ensure!(!line.contains(['\n', '\r']), "unexpected SDP helper output");
    let mut fields = line.split('\t');
    ensure!(
        fields.next() == Some("CHANNELS"),
        "invalid SDP helper response"
    );
    let mut channels = Vec::new();
    for field in fields {
        let channel = Channel::try_from(field.parse::<u8>()?)?;
        ensure!(!channels.contains(&channel), "duplicate SDP channel");
        channels.push(channel);
    }
    Ok(channels)
}

async fn read_discovery_channels(output: &mut (impl AsyncRead + Unpin)) -> Result<Vec<Channel>> {
    let mut bytes = Vec::new();
    output
        .take((DISCOVERY_OUTPUT_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    parse_discovery_channels(&bytes)
}

pub(crate) async fn discovery_channels(address: Address) -> Vec<Channel> {
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        // The process guard also owns cancellation while Bluetooth/TCC or the
        // OS SDP query is pending. No blocking query survives in this process.
        let (_process, mut output) = start(&["discover".into(), address.to_string()]).await?;
        read_discovery_channels(&mut output).await
    })
    .await;
    result.ok().and_then(Result::ok).unwrap_or_default()
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
    output: BufReader<tokio::net::UnixStream>,
}

pub async fn listen(channel: Option<Channel>) -> Result<Listener> {
    let directory = std::sync::Arc::new(tempfile::tempdir()?);
    let path = directory.path().join("stream");
    let socket = tokio::net::UnixListener::bind(&path)?;
    let (process, mut output) = start(&[
        "host".into(),
        path.to_str().context("socket path is not UTF-8")?.into(),
        channel.map_or(0, Channel::get).to_string(),
    ])
    .await?;
    let value = line(&mut output).await?;
    let fields: Vec<_> = value.split('\t').collect();
    ensure!(
        fields.len() == 3 && fields[0] == "READY",
        "invalid Bluetooth helper response: {value}"
    );
    let address = fields[1].parse()?;
    let channel = super::allocated_channel(channel, fields[2].parse()?)?.get();
    Ok(Listener {
        address,
        channel,
        socket,
        process: std::sync::Arc::new(process),
        directory,
        output,
    })
}

async fn accept_stream(
    socket: &tokio::net::UnixListener,
    output: &mut BufReader<tokio::net::UnixStream>,
) -> Result<tokio::net::UnixStream> {
    let mut message = String::new();
    tokio::select! {
        biased;
        result = output.read_line(&mut message) => {
            let length = result?;
            ensure!(length > 0, "Bluetooth helper exited while listening");
            ensure!(length < 4096, "invalid Bluetooth helper response while listening");
            if let Some(error) = message.trim_end().strip_prefix("ERROR\t") {
                bail!("Bluetooth helper: {error}");
            }
            bail!("unexpected Bluetooth helper response while listening: {}", message.trim_end());
        }
        result = socket.accept() => Ok(result?.0),
    }
}

impl Listener {
    pub async fn accept(&mut self) -> Result<Connection> {
        let socket = accept_stream(&self.socket, &mut self.output).await?;
        Ok(Box::new(Bridge {
            socket,
            _control: None,
            _process: self.process.clone(),
            _directory: self.directory.clone(),
        }))
    }
}

pub async fn connect(endpoint: &Contact) -> Result<Connection> {
    let directory = std::sync::Arc::new(tempfile::tempdir()?);
    let path = directory.path().join("stream");
    let socket = tokio::net::UnixListener::bind(&path)?;
    let (process, mut output) = start(&[
        "connect".into(),
        endpoint.address.to_string(),
        endpoint.channel.get().to_string(),
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
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::{ffi::OsStrExt, net::UnixStream},
        },
        path::Path,
        process::Command,
        time::Duration,
    };
    use tokio::io::AsyncWriteExt;

    unsafe extern "C" {
        fn blugit_test_dispatch_read(fd: i32, buffer: *mut u8, size: usize) -> isize;
        fn blugit_test_reserve_channel(
            directory: *const libc::c_char,
            requested: i32,
            selected: *mut i32,
        ) -> i32;
        fn blugit_test_sdp_publication_generation(type_descriptor: *mut u32, size: *mut u32)
        -> u64;
    }

    #[test]
    fn discovery_control_response_preserves_order_and_rejects_invalid_candidates() {
        let channels = super::parse_discovery_channels(b"CHANNELS\t30\t10\t1\n").unwrap();
        assert_eq!(
            channels
                .iter()
                .map(|channel| channel.get())
                .collect::<Vec<_>>(),
            [30, 10, 1]
        );
        assert!(
            super::parse_discovery_channels(b"CHANNELS\n")
                .unwrap()
                .is_empty()
        );
        let all_channels = format!(
            "CHANNELS{}\n",
            (1..=30)
                .map(|number| format!("\t{number}"))
                .collect::<String>()
        );
        assert_eq!(
            super::parse_discovery_channels(all_channels.as_bytes())
                .unwrap()
                .len(),
            30
        );
        for response in [
            &b"CHANNELS\t0\n"[..],
            &b"CHANNELS\t31\n"[..],
            &b"CHANNELS\t256\n"[..],
            &b"CHANNELS\t2\t2\n"[..],
            &b"CHANNELS\t2\nCHANNELS\t3\n"[..],
            &b"CHANNELS\t2"[..],
            &b"READY\n"[..],
            &b"CHANNELS\t\xff\n"[..],
        ] {
            assert!(
                super::parse_discovery_channels(response).is_err(),
                "{response:?}"
            );
        }
    }

    #[tokio::test]
    async fn discovery_control_reader_bounds_output_before_peer_closes() -> anyhow::Result<()> {
        let (mut writer, mut reader) = tokio::io::duplex(256);
        writer.write_all(&[b'x'; 256]).await?;
        let error = tokio::time::timeout(
            Duration::from_secs(1),
            super::read_discovery_channels(&mut reader),
        )
        .await?
        .unwrap_err();
        assert!(error.to_string().contains("too large"));
        drop(writer);
        Ok(())
    }

    #[test]
    fn sdp_publication_generation_uses_unsigned_eight_byte_epoch_milliseconds() -> anyhow::Result<()>
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let before = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let (mut type_descriptor, mut size) = (0, 0);
        // SAFETY: both output pointers are valid for the native SDK conversion.
        let generation =
            unsafe { blugit_test_sdp_publication_generation(&mut type_descriptor, &mut size) };
        let after = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        assert_eq!((type_descriptor, size), (1, 8));
        assert!(generation > u64::from(u32::MAX));
        assert!((before..=after).contains(&u128::from(generation)));
        Ok(())
    }

    #[tokio::test]
    async fn standalone_helper_rejects_public_cli_arguments_without_initializing_the_radio()
    -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir()?;
        let helper = directory.path().join("blugit-bluetooth");
        std::fs::write(
            &helper,
            include_bytes!(concat!(env!("OUT_DIR"), "/blugit-bluetooth")),
        )?;
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755))?;
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new(&helper)
                .arg("unexpected-public-command")
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

    fn reserve(directory: &Path, requested: i32) -> io::Result<(OwnedFd, i32)> {
        let directory = std::ffi::CString::new(directory.as_os_str().as_bytes())?;
        let mut selected = 0;
        // SAFETY: the directory and output pointer remain live during the call.
        let fd =
            unsafe { blugit_test_reserve_channel(directory.as_ptr(), requested, &mut selected) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the native allocator transfers this newly opened fd to us.
        Ok((unsafe { OwnedFd::from_raw_fd(fd) }, selected))
    }

    #[test]
    fn channel_reservations_reject_conflicts_in_another_process_and_release_on_close()
    -> io::Result<()> {
        const CHILD_DIRECTORY: &str = "BLUGIT_TEST_CHANNEL_RESERVATION_DIRECTORY";
        if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
            assert_eq!(
                reserve(Path::new(&directory), 4)
                    .unwrap_err()
                    .raw_os_error(),
                Some(libc::EADDRINUSE)
            );
            return Ok(());
        }
        let directory = tempfile::tempdir()?;
        let (owner, selected) = reserve(directory.path(), 4)?;
        assert_eq!(selected, 4);
        let output = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "platform::macos::tests::channel_reservations_reject_conflicts_in_another_process_and_release_on_close",
            ])
            .env(CHILD_DIRECTORY, directory.path())
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed;"));
        assert_eq!(
            reserve(directory.path(), 4).unwrap_err().raw_os_error(),
            Some(libc::EADDRINUSE)
        );
        drop(owner);
        assert_eq!(reserve(directory.path(), 4)?.1, 4);
        Ok(())
    }

    #[test]
    fn automatic_channel_reservations_are_distinct_and_report_exhaustion() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let mut owners = Vec::new();
        for expected in 1..=30 {
            let (owner, selected) = reserve(directory.path(), 0)?;
            assert_eq!(selected, expected);
            owners.push(owner);
        }
        assert_eq!(
            reserve(directory.path(), 0).unwrap_err().raw_os_error(),
            Some(libc::EADDRINUSE)
        );
        drop(owners.remove(6));
        assert_eq!(reserve(directory.path(), 0)?.1, 7);
        Ok(())
    }

    #[tokio::test]
    async fn listener_reports_native_failure_and_exit_instead_of_waiting_forever()
    -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let listener = tokio::net::UnixListener::bind(directory.path().join("listener"))?;
        for notification in [Some("ERROR\tBluetooth stopped\n"), None] {
            let (reader, mut helper) = tokio::net::UnixStream::pair()?;
            let mut output = tokio::io::BufReader::new(reader);
            if let Some(notification) = notification {
                helper.write_all(notification.as_bytes()).await?;
            }
            drop(helper);
            let error = tokio::time::timeout(
                Duration::from_secs(1),
                super::accept_stream(&listener, &mut output),
            )
            .await?
            .unwrap_err();
            assert!(error.to_string().contains(if notification.is_some() {
                "Bluetooth stopped"
            } else {
                "helper exited"
            }));
        }
        Ok(())
    }

    #[test]
    fn native_helper_is_separate_from_the_terminal_signal_group() -> io::Result<()> {
        let (_reader, output) = UnixStream::pair()?;
        let mut arguments = [
            c"/bin/sleep".as_ptr().cast_mut(),
            c"30".as_ptr().cast_mut(),
            std::ptr::null_mut(),
        ];
        let mut pid = 0;
        // SAFETY: argv is null-terminated, and the socket and C strings survive spawn.
        let error = unsafe {
            super::blugit_spawn(
                c"/bin/sleep".as_ptr(),
                arguments.as_mut_ptr(),
                output.as_raw_fd(),
                &mut pid,
            )
        };
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error));
        }
        let _child = super::Process { pid };
        // SAFETY: this pid belongs to the child kept alive by the process guard.
        let child_group = unsafe { libc::getpgid(pid) };
        // SAFETY: getpgrp reads this process's signal group.
        let parent_group = unsafe { libc::getpgrp() };
        assert_eq!(child_group, pid, "the helper must own its signal group");
        assert_ne!(
            child_group, parent_group,
            "terminal Ctrl+C must reach the Rust CLI only"
        );
        Ok(())
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
