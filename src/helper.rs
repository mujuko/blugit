use crate::{
    endpoint::Endpoint,
    platform,
    wire::{self, CHUNK, Kind},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    io::{Read, Write},
    time::Duration,
};

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
    let endpoint: Endpoint = args
        .last()
        .ok_or_else(|| anyhow::anyhow!("missing remote URL"))?
        .parse()?;
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
            b"connect git-upload-pack" => return transfer(&endpoint, b"git-upload-pack").await,
            b"connect git-receive-pack" => return transfer(&endpoint, b"git-receive-pack").await,
            b"" => return Ok(0),
            _ => bail!(
                "unsupported helper command: {}",
                String::from_utf8_lossy(&command)
            ),
        }
    }
}

async fn transfer(endpoint: &Endpoint, service: &[u8]) -> Result<u8> {
    let mut stream = platform::connect(endpoint)
        .await
        .context("connect Bluetooth transport")?;
    wire::send(&mut stream, Kind::Request, service)
        .await
        .context("send initial REQUEST to host")?;
    let (kind, data) = tokio::time::timeout(Duration::from_secs(15), wire::receive(&mut stream))
        .await
        .context("timed out waiting for READY")?
        .context("receive READY from host")?;
    if kind == Kind::Error {
        bail!("{}", String::from_utf8_lossy(&data));
    }
    ensure!(
        kind == Kind::Ready && data.is_empty(),
        "invalid host response"
    );
    std::io::stdout().write_all(b"\n")?;
    std::io::stdout().flush()?;

    let (mut reader, mut writer) = tokio::io::split(stream);
    let (sender, mut input) = tokio::sync::mpsc::channel::<std::io::Result<Vec<u8>>>(2);
    // A detached OS thread exits with the process if Git keeps stdin open.
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
    let (done_sender, mut done) = tokio::sync::oneshot::channel();
    let outgoing = async {
        let mut input_open = true;
        loop {
            // Once a frame write starts, finish it before observing EXIT. An ACK
            // must never interrupt the middle of a DATA frame on the stream.
            tokio::select! {
                _ = &mut done => break,
                data = input.recv(), if input_open => {
                    let data = data.ok_or_else(|| anyhow::anyhow!("stdin reader stopped"))??;
                    input_open = !data.is_empty();
                    wire::send(&mut writer, if input_open { Kind::Data } else { Kind::Eof }, &data).await?;
                }
            }
        }
        wire::send(&mut writer, Kind::Ack, b"").await?;
        Ok::<_, anyhow::Error>(())
    };
    let incoming = async move {
        loop {
            let (kind, data) = wire::receive(&mut reader)
                .await
                .context("receive Git output/EXIT from host")?;
            match kind {
                Kind::Data => {
                    let mut stdout = std::io::stdout().lock();
                    stdout.write_all(&data)?;
                    stdout.flush()?;
                }
                Kind::Stderr | Kind::Error => {
                    let mut stderr = std::io::stderr().lock();
                    stderr.write_all(&data)?;
                    stderr.flush()?;
                    if kind == Kind::Error {
                        bail!("host reported an error");
                    }
                }
                Kind::Exit => {
                    let code: i32 = std::str::from_utf8(&data)?.parse()?;
                    let _ = done_sender.send(());
                    return Ok::<_, anyhow::Error>(u8::from(code != 0));
                }
                _ => bail!("unexpected host frame"),
            }
        }
    };
    let (code, ()) = tokio::try_join!(incoming, outgoing)?;
    Ok(code)
}
