use crate::{platform::CHUNK, storage::Alias};
use anyhow::{Result, bail, ensure};
use std::io::{Read, Write};

pub async fn run() -> Result<u8> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        println!("git-remote-blugit {}", env!("CARGO_PKG_VERSION"));
        return Ok(0);
    }
    ensure!(
        (1..=2).contains(&args.len()),
        "git-remote-blugit is invoked by Git"
    );
    let name = Alias::from_url(
        args.last()
            .ok_or_else(|| anyhow::anyhow!("missing remote URL"))?,
    )?;
    let mut input = std::io::stdin();
    loop {
        // Read only the command bytes. Buffered readers can steal the initial
        // bytes of the later binary Git protocol from the relay thread.
        let mut command = Vec::new();
        loop {
            let mut byte = [0];
            if input.read(&mut byte)? == 0 {
                return Ok(0);
            }
            if byte[0] == b'\n' {
                break;
            }
            ensure!(command.len() < 1024, "helper command too long");
            command.push(byte[0]);
        }
        match command.as_slice() {
            b"capabilities" => {
                let mut output = std::io::stdout().lock();
                output.write_all(b"connect\n\n")?;
                output.flush()?;
            }
            b"connect git-upload-pack" => return transfer(&name, "git-upload-pack").await,
            b"connect git-receive-pack" => return transfer(&name, "git-receive-pack").await,
            b"" => return Ok(0),
            _ => bail!(
                "unsupported helper command: {}",
                String::from_utf8_lossy(&command)
            ),
        }
    }
}

async fn transfer(name: &Alias, service: &str) -> Result<u8> {
    let session = crate::client::connect(name).await?;
    let channel = session.exec(service).await?;
    std::io::stdout().write_all(b"\n")?;
    std::io::stdout().flush()?;
    let (mut reader, writer) = channel.split();
    let (sender, mut input) = tokio::sync::mpsc::channel::<std::io::Result<Vec<u8>>>(2);
    // A detached OS thread exits with the helper if Git keeps stdin open.
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin();
        loop {
            let mut buffer = vec![0; CHUNK];
            match stdin.read(&mut buffer) {
                Ok(count) => {
                    buffer.truncate(count);
                    if sender.blocking_send(Ok(buffer)).is_err() || count == 0 {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.blocking_send(Err(error));
                    return;
                }
            }
        }
    });
    let outgoing = async {
        while let Some(data) = input.recv().await {
            let data = data?;
            if data.is_empty() {
                writer.eof().await?;
                break;
            }
            writer.data(data.as_slice()).await?;
        }
        std::future::pending::<Result<()>>().await
    };
    let incoming = async {
        let mut status = None;
        let mut eof = false;
        while let Some(message) = reader.wait().await {
            match message {
                russh::ChannelMsg::Data { data } => {
                    let mut output = std::io::stdout().lock();
                    output.write_all(&data)?;
                    output.flush()?;
                }
                russh::ChannelMsg::ExtendedData { data, ext: 1 } => {
                    let mut output = std::io::stderr().lock();
                    output.write_all(&data)?;
                    output.flush()?;
                }
                russh::ChannelMsg::ExitStatus { exit_status } => {
                    ensure!(status.is_none(), "duplicate SSH exit status");
                    status = Some(exit_status);
                }
                russh::ChannelMsg::Eof => eof = true,
                russh::ChannelMsg::ExitSignal { .. } => bail!("remote Git terminated by a signal"),
                russh::ChannelMsg::Close if eof && status.is_some() => return Ok(status.unwrap()),
                russh::ChannelMsg::Close => {
                    bail!("SSH channel closed before Git exit status and EOF")
                }
                _ => {}
            }
        }
        bail!("SSH disconnected before Git completed")
    };
    tokio::pin!(outgoing, incoming);
    let status = tokio::select! {
        result = &mut outgoing => { result?; unreachable!() },
        result = &mut incoming => result?,
    };
    session.finish().await?;
    Ok(u8::from(status != 0))
}
