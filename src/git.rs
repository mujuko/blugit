use crate::{
    platform::Connection,
    wire::{self, CHUNK, Kind},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::Mutex,
};

fn git_command() -> Command {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
}

pub async fn validate_repository(path: &Path) -> Result<PathBuf> {
    let path = dunce::canonicalize(path).context("resolve repository path")?;
    let output = git_command()
        .current_dir(&path)
        .args(["rev-parse", "--is-bare-repository"])
        .output()
        .await
        .context("run Git (is Git installed?)")?;
    ensure!(
        output.status.success() && output.stdout == b"true\n",
        "host requires a bare repository: git init --bare <path>"
    );
    Ok(path)
}

pub async fn serve(stream: Connection, repository: &Path) -> Result<()> {
    let (mut reader, writer) = tokio::io::split(stream);
    let writer = Arc::new(Mutex::new(writer));
    let (kind, data) = tokio::time::timeout(Duration::from_secs(15), wire::receive(&mut reader))
        .await
        .context("timed out waiting for initial REQUEST")?
        .context("receive initial REQUEST before starting Git")?;
    let service = match (kind, data.as_slice()) {
        (Kind::Request, b"git-upload-pack") => "upload-pack",
        (Kind::Request, b"git-receive-pack") => "receive-pack",
        _ => {
            wire::send(
                &mut *writer.lock().await,
                Kind::Error,
                b"expected git-upload-pack or git-receive-pack",
            )
            .await?;
            return Ok(());
        }
    };
    let mut child = git_command()
        .arg(service)
        .arg(repository)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("start git {service}"))?;
    let mut stdin = child.stdin.take().context("Git stdin")?;
    let stdout = child.stdout.take().context("Git stdout")?;
    let stderr = child.stderr.take().context("Git stderr")?;
    wire::send(&mut *writer.lock().await, Kind::Ready, b"")
        .await
        .context("send READY to guest")?;

    async fn forward<R: AsyncRead + Unpin>(
        mut input: R,
        writer: Arc<Mutex<tokio::io::WriteHalf<Connection>>>,
        kind: Kind,
    ) -> Result<()> {
        let mut buffer = vec![0; CHUNK];
        loop {
            let count = input.read(&mut buffer).await?;
            if count == 0 {
                return Ok(());
            }
            wire::send(&mut *writer.lock().await, kind, &buffer[..count]).await?;
        }
    }
    let exited = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let incoming_exited = exited.clone();
    let incoming = async move {
        let mut eof = false;
        loop {
            let (kind, data) = wire::receive(&mut reader).await.with_context(|| {
                if incoming_exited.load(std::sync::atomic::Ordering::Acquire) {
                    format!("receive final ACK after git {service} exited")
                } else {
                    format!("receive guest DATA/EOF for git {service}")
                }
            })?;
            match kind {
                Kind::Ack
                    if data.is_empty()
                        && incoming_exited.load(std::sync::atomic::Ordering::Acquire) =>
                {
                    return Ok::<_, anyhow::Error>(());
                }
                Kind::Eof if data.is_empty() && !eof => {
                    stdin.shutdown().await?;
                    eof = true;
                }
                Kind::Data if !eof => {
                    if let Err(error) = stdin.write_all(&data).await {
                        if error.kind() != std::io::ErrorKind::BrokenPipe {
                            return Err(error.into());
                        }
                        eof = true;
                    }
                }
                _ => bail!("unexpected guest frame"),
            }
        }
    };
    let outgoing = async {
        tokio::try_join!(
            async {
                forward(stdout, writer.clone(), Kind::Data)
                    .await
                    .context("forward Git stdout to guest")
            },
            async {
                forward(stderr, writer.clone(), Kind::Stderr)
                    .await
                    .context("forward Git stderr to guest")
            }
        )?;
        let status = child.wait().await?;
        exited.store(true, std::sync::atomic::Ordering::Release);
        wire::send(
            &mut *writer.lock().await,
            Kind::Exit,
            status.code().unwrap_or(1).to_string().as_bytes(),
        )
        .await
        .context("send EXIT to guest")?;
        Ok::<_, anyhow::Error>(())
    };
    tokio::pin!(incoming, outgoing);
    // Do not put receive() directly inside a select loop: cancelling half-read
    // frames when Git exits would corrupt the remaining acknowledgement stream.
    let result = tokio::select! {
        result = &mut incoming => {
            result?;
            tokio::time::timeout(Duration::from_secs(10), &mut outgoing).await?
        }
        result = &mut outgoing => {
            result?;
            tokio::time::timeout(Duration::from_secs(10), &mut incoming).await?
        }
    };
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disconnect_before_request_identifies_handshake_phase() {
        let (host, guest) = tokio::io::duplex(16);
        drop(guest);
        let error = serve(Box::new(host), Path::new("."))
            .await
            .expect_err("guest disconnected");
        let message = format!("{error:#}");
        assert!(
            message.contains("receive initial REQUEST before starting Git"),
            "{message}"
        );
        assert!(message.contains("read frame type"), "{message}");
    }
}
