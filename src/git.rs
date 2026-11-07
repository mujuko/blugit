use crate::model::ShareId;
use anyhow::{Context, Result, bail, ensure};
use russh::{Channel, ChannelMsg};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
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

/// Parse a restricted SSH exec without invoking a shell or accepting a path.
pub(crate) fn parse_exec(command: &[u8], share: ShareId) -> Option<&'static str> {
    for (service, subcommand) in [
        ("git-upload-pack", "upload-pack"),
        ("git-receive-pack", "receive-pack"),
    ] {
        if command == format!("{service} '{share}'").as_bytes() {
            return Some(subcommand);
        }
    }
    None
}

pub(crate) fn spawn(service: &str, repository: &Path) -> Result<Child> {
    let mut command = git_command();
    // Ctrl+C belongs to the host's drain/abort policy. Letting the terminal
    // broadcast it to Git (and its hooks) would interrupt an authorized transfer.
    #[cfg(windows)]
    command.creation_flags(0x0000_0200); // CREATE_NEW_PROCESS_GROUP
    #[cfg(unix)]
    command.process_group(0);
    command
        .arg(service)
        .arg(repository)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("start git {service}"))
}

pub(crate) async fn serve(channel: Channel<russh::server::Msg>, mut child: Child) -> Result<()> {
    let (mut input, output) = channel.split();
    let mut stdin = Some(child.stdin.take().context("Git stdin")?);
    let stdout = child.stdout.take().context("Git stdout")?;
    let stderr = child.stderr.take().context("Git stderr")?;
    let incoming = async {
        let mut eof = false;
        while let Some(message) = input.wait().await {
            match message {
                ChannelMsg::Data { data } => {
                    ensure!(!eof, "Git data received after EOF");
                    if let Some(pipe) = stdin.as_mut()
                        && let Err(error) = pipe.write_all(&data).await
                    {
                        if error.kind() != std::io::ErrorKind::BrokenPipe {
                            return Err(error.into());
                        }
                        stdin.take();
                    }
                }
                ChannelMsg::Eof => {
                    eof = true;
                    if let Some(mut pipe) = stdin.take() {
                        pipe.shutdown().await?;
                        drop(pipe);
                    }
                }
                ChannelMsg::Close => {
                    bail!("guest closed SSH channel before Git completed");
                }
                _ => {}
            }
        }
        bail!("SSH disconnected before Git completed")
    };
    async fn forward<R: AsyncRead + Unpin>(
        mut source: R,
        output: &russh::ChannelWriteHalf<russh::server::Msg>,
        stderr: bool,
    ) -> Result<()> {
        let mut buffer = vec![0; crate::platform::CHUNK];
        loop {
            let count = source.read(&mut buffer).await?;
            if count == 0 {
                return Ok(());
            }
            if stderr {
                output.extended_data(1, &buffer[..count]).await?;
            } else {
                output.data(&buffer[..count]).await?;
            }
        }
    }
    let outgoing = async {
        tokio::try_join!(
            forward(stdout, &output, false),
            forward(stderr, &output, true)
        )?;
        let status = child.wait().await?;
        output
            .exit_status(status.code().unwrap_or(1) as u32)
            .await?;
        output.eof().await?;
        output.close().await?;
        Ok::<_, anyhow::Error>(())
    };
    tokio::pin!(incoming, outgoing);
    tokio::select! {
        result = &mut incoming => result?,
        result = &mut outgoing => result?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exec_accepts_only_git_services_for_the_hosted_share() {
        let share = ShareId::generate();
        assert_eq!(
            parse_exec(format!("git-upload-pack '{share}'").as_bytes(), share),
            Some("upload-pack")
        );
        assert_eq!(
            parse_exec(format!("git-receive-pack '{share}'").as_bytes(), share),
            Some("receive-pack")
        );
        for command in [
            format!("git-upload-pack '{}'", ShareId::generate()),
            "git-upload-pack '../other.git'".into(),
            format!("git-upload-pack '{share}'; echo injected"),
            "sh".into(),
        ] {
            assert_eq!(parse_exec(command.as_bytes(), share), None);
        }
    }
}
