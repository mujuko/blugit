use super::*;
use crate::{endpoint::Channel as RfcommChannel, model::ShareId};
use tokio::time::timeout;

struct Fixture {
    _dir: tempfile::TempDir,
    host: Device,
    guest: Device,
    share: ShareStore,
    endpoint: Endpoint,
}
impl Fixture {
    fn new() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let repo = dir.path().join("host repo.git");
        ensure!(
            std::process::Command::new("git")
                .args(["init", "--bare", "--initial-branch=main"])
                .arg(&repo)
                .output()?
                .status
                .success(),
            "git init failed"
        );
        let host = Device::open(dir.path().join("host"), Some("Host".into()))?;
        let guest = Device::open(dir.path().join("guest"), Some("Guest".into()))?;
        let share = ShareStore::register(host.root().into(), &repo)?;
        share.grant(guest.peer())?;
        let endpoint = Endpoint {
            address: "AA:BB:CC:DD:EE:FF".parse()?,
            channel: RfcommChannel::try_from(7)?,
            share: share.id(),
        };
        TrustStore::new(guest.root().into()).pin(&endpoint, host.peer())?;
        Ok(Self {
            _dir: dir,
            host,
            guest,
            share,
            endpoint,
        })
    }
    fn start(
        &self,
    ) -> (
        Connection,
        watch::Sender<Shutdown>,
        tokio::task::JoinHandle<Result<()>>,
    ) {
        let (a, b) = tokio::io::duplex(4096);
        let (stop, shutdown) = watch::channel(Shutdown::Running);
        let host = self.host.clone();
        let share = self.share.clone();
        let task = tokio::spawn(async move { serve(Box::new(a), &host, share, shutdown).await });
        (Box::new(b), stop, task)
    }
}

async fn complete(
    mut channel: Channel<russh::client::Msg>,
    session: Session,
) -> Result<(u32, Vec<u8>, Vec<u8>)> {
    channel.eof().await?;
    timeout(Duration::from_secs(5), async {
        let mut status = None;
        let mut eof = false;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                ChannelMsg::ExtendedData { data, ext: 1 } => stderr.extend_from_slice(&data),
                ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
                ChannelMsg::Eof => eof = true,
                ChannelMsg::Close if eof && status.is_some() => {
                    session.finish().await?;
                    return Ok((status.unwrap(), stdout, stderr));
                }
                ChannelMsg::Close => bail!("SSH closed before exit status and EOF"),
                _ => {}
            }
        }
        bail!("SSH closed before completion")
    })
    .await?
}

#[tokio::test]
async fn ssh_carries_git_output_stderr_and_nonzero_exit_without_wire_frames() -> Result<()> {
    let f = Fixture::new()?;
    let (stream, _stop, task) = f.start();
    let session = connect(stream, &f.guest, &f.endpoint).await?;
    let channel = session.exec("git-upload-pack").await?;
    // EOF without a Git request makes upload-pack report a real protocol error.
    let (status, stdout, stderr) = complete(channel, session).await?;
    ensure!(
        status != 0 && !stdout.is_empty() && !stderr.is_empty(),
        "missing Git output/error/status"
    );
    timeout(Duration::from_secs(2), task).await???;
    Ok(())
}

#[tokio::test]
async fn changed_ssh_host_key_is_rejected_even_when_enrollment_key_matches() -> Result<()> {
    let f = Fixture::new()?;
    let mut pinned = f.host.peer().clone();
    pinned.ssh_key = f.guest.peer().ssh_key;
    TrustStore::new(f.guest.root().into()).pin(&f.endpoint, &pinned)?;
    let (stream, _stop, task) = f.start();
    let error = connect(stream, &f.guest, &f.endpoint)
        .await
        .err()
        .context("changed key accepted")?;
    ensure!(
        format!("{error:#}").contains("host key changed"),
        "wrong error: {error:#}"
    );
    let _ = timeout(Duration::from_secs(2), task).await?;
    Ok(())
}

#[tokio::test]
async fn ssh_authentication_is_bound_to_the_registered_repository() -> Result<()> {
    let f = Fixture::new()?;
    let mut wrong = f.endpoint.clone();
    wrong.share = ShareId::generate();
    TrustStore::new(f.guest.root().into()).pin(&wrong, f.host.peer())?;
    let (stream, _stop, task) = f.start();
    let error = connect(stream, &f.guest, &wrong)
        .await
        .err()
        .context("authenticated the wrong repository")?;
    ensure!(
        error.to_string().contains("share ID does not match"),
        "{error:#}"
    );
    let _ = timeout(Duration::from_secs(2), task).await?;
    Ok(())
}

#[tokio::test]
async fn revoked_devices_cannot_authenticate_or_start_a_new_exec() -> Result<()> {
    let f = Fixture::new()?;
    let (stream, _stop, task) = f.start();
    let session = connect(stream, &f.guest, &f.endpoint).await?;
    f.share.revoke(&f.guest.peer().id())?;
    assert!(session.exec("git-receive-pack").await.is_err());
    drop(session);
    let _ = timeout(Duration::from_secs(2), task).await?;
    let (stream, _stop, task) = f.start();
    assert!(connect(stream, &f.guest, &f.endpoint).await.is_err());
    let _ = timeout(Duration::from_secs(2), task).await?;
    Ok(())
}

#[tokio::test]
async fn device_id_and_display_name_cannot_substitute_for_the_ssh_private_key() -> Result<()> {
    let f = Fixture::new()?;
    let stranger = Device::open(
        f._dir.path().join("stranger"),
        Some(f.guest.peer().name.clone()),
    )?;
    let (stream, _stop, task) = f.start();
    let (stream, _cancel) = CancelStream::wrap(stream);
    let mut client = russh::client::connect_stream(
        Arc::new(Default::default()),
        stream,
        Client {
            host_key: f.host.peer().ssh_key,
        },
    )
    .await?;
    let result = client
        .authenticate_publickey(
            username(f.endpoint.share, f.guest.peer()),
            PrivateKeyWithHashAlg::new(Arc::new(stranger.ssh_private_key()), None),
        )
        .await?;
    assert!(!result.success());
    drop(_cancel);
    let _ = timeout(Duration::from_secs(2), task).await?;
    Ok(())
}

#[tokio::test]
async fn wrong_share_shell_forwarding_and_extra_channels_are_refused() -> Result<()> {
    let f = Fixture::new()?;
    let (stream, _stop, task) = f.start();
    let session = connect(stream, &f.guest, &f.endpoint).await?;
    assert!(
        session
            .handle
            .channel_open_direct_tcpip("localhost", 22, "localhost", 0)
            .await
            .is_err()
    );
    let mut channel = session.handle.channel_open_session().await?;
    assert!(session.handle.channel_open_session().await.is_err());
    channel.request_shell(true).await?;
    assert!(matches!(channel.wait().await, Some(ChannelMsg::Failure)));
    for command in [
        format!("git-upload-pack '{}'", ShareId::generate()),
        format!("git-upload-pack '{}'; echo injected", f.share.id()),
    ] {
        channel.exec(true, command).await?;
        assert!(matches!(
            channel.wait().await,
            Some(ChannelMsg::ExtendedData { ext: 1, .. })
        ));
        assert!(matches!(channel.wait().await, Some(ChannelMsg::Failure)));
    }
    channel
        .exec(true, format!("git-upload-pack '{}'", f.share.id()))
        .await?;
    assert!(matches!(channel.wait().await, Some(ChannelMsg::Success)));
    // Repeated exec must not spawn another Git process in an active channel.
    channel
        .exec(true, format!("git-receive-pack '{}'", f.share.id()))
        .await?;
    let mut refused = false;
    while let Some(message) = channel.wait().await {
        if matches!(message, ChannelMsg::Failure) {
            refused = true;
            break;
        }
    }
    assert!(refused);
    complete(channel, session).await?;
    timeout(Duration::from_secs(2), task).await???;
    Ok(())
}

#[tokio::test]
async fn revocation_after_exec_allows_the_authorized_transfer_to_finish() -> Result<()> {
    let f = Fixture::new()?;
    let (stream, _stop, task) = f.start();
    let session = connect(stream, &f.guest, &f.endpoint).await?;
    let channel = session.exec("git-upload-pack").await?;
    f.share.revoke(&f.guest.peer().id())?;
    complete(channel, session).await?;
    timeout(Duration::from_secs(2), task).await???;
    Ok(())
}

#[tokio::test]
async fn stop_cancels_authenticated_connections_waiting_for_exec() -> Result<()> {
    let f = Fixture::new()?;
    let (stream, stop, task) = f.start();
    let session = connect(stream, &f.guest, &f.endpoint).await?;
    stop.send(Shutdown::Drain)?;
    timeout(Duration::from_secs(2), task).await???;
    assert!(
        timeout(Duration::from_secs(2), session.exec("git-upload-pack"))
            .await?
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn buffered_radio_bridge_delivers_final_ssh_close_before_it_is_dropped() -> Result<()> {
    let f = Fixture::new()?;
    let (a, b) = tokio::io::duplex(67);
    let (_stop, shutdown) = watch::channel(Shutdown::Running);
    let host = f.host.clone();
    let share = f.share.clone();
    let task = tokio::spawn(async move {
        serve(
            crate::test_support::BufferedBridge::wrap(a),
            &host,
            share,
            shutdown,
        )
        .await
    });
    let session = connect(
        crate::test_support::BufferedBridge::wrap(b),
        &f.guest,
        &f.endpoint,
    )
    .await?;
    let channel = session.exec("git-upload-pack").await?;
    complete(channel, session).await?;
    timeout(Duration::from_secs(2), task).await???;
    Ok(())
}

#[tokio::test]
async fn abort_stops_an_active_git_session_and_its_ssh_transport() -> Result<()> {
    let f = Fixture::new()?;
    let (stream, stop, task) = f.start();
    let session = connect(stream, &f.guest, &f.endpoint).await?;
    let mut channel = session.exec("git-upload-pack").await?;
    stop.send(Shutdown::Abort)?;
    timeout(Duration::from_secs(2), task).await???;
    timeout(Duration::from_secs(2), async {
        while let Some(message) = channel.wait().await {
            assert!(
                !matches!(message, ChannelMsg::ExitStatus { exit_status: 0 }),
                "aborted transfer reported success"
            );
        }
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn premature_channel_close_cancels_git_without_waiting_for_more_input() -> Result<()> {
    let f = Fixture::new()?;
    let (stream, _stop, task) = f.start();
    let session = connect(stream, &f.guest, &f.endpoint).await?;
    let channel = session.exec("git-upload-pack").await?;
    channel.close().await?;
    assert!(timeout(Duration::from_secs(2), task).await??.is_err());
    assert!(session.finish().await.is_err());
    Ok(())
}

#[tokio::test]
async fn openssh_client_can_execute_the_git_service_with_a_pinned_host_key() -> Result<()> {
    match tokio::process::Command::new("ssh").arg("-V").output().await {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("OpenSSH interoperability test skipped: ssh is not installed");
            return Ok(());
        }
        result => {
            result?;
        }
    }
    let f = Fixture::new()?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let key_path = f._dir.path().join("id_ed25519");
    let hosts_path = f._dir.path().join("known_hosts");
    let private = f
        .guest
        .ssh_private_key()
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)?;
    {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&key_path)?.write_all(private.as_bytes())?;
    }
    #[cfg(windows)]
    {
        // OpenSSH rejects inherited read access for other Windows accounts.
        // Restrict only the disposable test key, using the actual process SID.
        let who = std::process::Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()?;
        ensure!(who.status.success(), "read test process SID");
        let identity = String::from_utf8(who.stdout)?;
        let sid = identity
            .trim()
            .rsplit(',')
            .next()
            .context("missing process SID")?
            .trim_matches('"');
        ensure!(sid.starts_with("S-1-"), "invalid test process SID");
        let acl = std::process::Command::new("icacls")
            .arg(&key_path)
            .args(["/inheritance:r", "/grant:r"])
            .arg(format!("*{sid}:R"))
            .output()?;
        ensure!(
            acl.status.success(),
            "restrict temporary OpenSSH key ACL: {}",
            String::from_utf8_lossy(&acl.stderr)
        );
    }
    std::fs::write(
        &hosts_path,
        format!(
            "[127.0.0.1]:{port} {}\n",
            f.host.ssh_private_key().public_key().to_openssh()?
        ),
    )?;
    let (_stop, shutdown) = watch::channel(Shutdown::Running);
    let host = f.host.clone();
    let share = f.share.clone();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        serve(Box::new(stream), &host, share, shutdown).await
    });
    let output = timeout(
        Duration::from_secs(10),
        tokio::process::Command::new("ssh")
            .args([
                "-F",
                "none",
                "-T",
                "-n",
                "-o",
                "BatchMode=yes",
                "-o",
                "IdentitiesOnly=yes",
                "-o",
                "IdentityAgent=none",
                "-o",
                "StrictHostKeyChecking=yes",
                "-o",
                "GlobalKnownHostsFile=none",
                "-o",
            ])
            .arg(format!("UserKnownHostsFile=\"{}\"", hosts_path.display()))
            .arg("-i")
            .arg(&key_path)
            .arg("-p")
            .arg(port.to_string())
            .arg("-l")
            .arg(username(f.endpoint.share, f.guest.peer()))
            .arg("127.0.0.1")
            .arg(format!("git-upload-pack '{}'", f.share.id()))
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    ensure!(
        output.status.code() == Some(128)
            && !output.stdout.is_empty()
            && String::from_utf8_lossy(&output.stderr).contains("fatal"),
        "OpenSSH did not receive Git output and status: {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    timeout(Duration::from_secs(2), task).await???;
    Ok(())
}
