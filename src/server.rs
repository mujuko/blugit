//! Bounded session management and graceful stop; no CLI or OS signal handling.
use crate::{
    platform::{Acceptor, Connection},
    protocol,
    storage::{Device, ShareStore},
};
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::{sync::watch, task::JoinSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shutdown {
    Running,
    Drain,
    Abort,
}
pub struct Host {
    auth: protocol::Server,
    device: Device,
    share: ShareStore,
}
impl Host {
    pub fn new(device: Device, share: ShareStore, notify: protocol::Notify) -> Self {
        Self {
            auth: protocol::Server::new(device.clone(), share.clone(), notify),
            device,
            share,
        }
    }
    async fn handle(
        &self,
        stream: Connection,
        mut shutdown: watch::Receiver<Shutdown>,
    ) -> Result<()> {
        if *shutdown.borrow() != Shutdown::Running {
            return Ok(());
        }
        use tokio::io::AsyncBufReadExt;
        let mut stream = tokio::io::BufReader::new(stream);
        let ssh = tokio::select! {
            biased;
            _=shutdown.changed()=>return Ok(()),
            result=tokio::time::timeout(std::time::Duration::from_secs(15), stream.fill_buf())=> {
                result.context("initial protocol identification timed out")??.starts_with(b"S")
            }
        };
        if ssh {
            crate::ssh::serve(Box::new(stream), &self.device, self.share.clone(), shutdown).await?;
        } else {
            tokio::select! {
                _=shutdown.changed()=>return Ok(()),
                result=self.auth.accept(Box::new(stream))=>result?,
            }
        }
        Ok(())
    }
}

pub async fn run<L: Acceptor>(
    mut listener: L,
    host: Arc<Host>,
    mut shutdown: watch::Receiver<Shutdown>,
) -> Result<()> {
    let mut tasks = JoinSet::new();
    let mut failure = None;
    loop {
        if *shutdown.borrow() != Shutdown::Running {
            break;
        }
        tokio::select! {
            biased;
            changed=shutdown.changed()=>{if changed.is_err(){tasks.abort_all();break;}},
            result=tasks.join_next(),if !tasks.is_empty()=>report(result),
            connection=listener.accept()=>match connection {
                Ok(stream) if tasks.len()<16=>{let host=host.clone();let shutdown=shutdown.clone();tasks.spawn(async move{host.handle(stream,shutdown).await});},
                Ok(_)=>eprintln!("Connection limit reached; try again later"),
                Err(error)=>{failure=Some(error);tasks.abort_all();break;},
            }
        }
    }
    // Dropping the listener refuses new sessions while established Git drains.
    drop(listener);
    eprintln!(
        "Sharing stopped; waiting for {} connection(s). Press Ctrl+C again to abort.",
        tasks.len()
    );
    while !tasks.is_empty() {
        if *shutdown.borrow() == Shutdown::Abort {
            tasks.abort_all();
        }
        tokio::select! {
            result=tasks.join_next()=>report(result),
            changed=shutdown.changed()=>{if changed.is_err(){tasks.abort_all();}},
        }
    }
    if let Some(error) = failure {
        return Err(error).context("Bluetooth listener failed");
    }
    Ok(())
}
fn report(result: Option<Result<Result<()>, tokio::task::JoinError>>) {
    match result {
        Some(Ok(Err(error))) => eprintln!("Connection ended: {error:#}"),
        Some(Err(error)) if !error.is_cancelled() => eprintln!("Connection task failed: {error}"),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    struct Empty;
    #[async_trait]
    impl Acceptor for Empty {
        async fn accept(&mut self) -> Result<Connection> {
            std::future::pending().await
        }
    }
    #[tokio::test]
    async fn idle_host_stops_without_waiting_for_an_accept() -> Result<()> {
        let d = tempfile::tempdir()?;
        let repo = d.path().join("repo");
        std::fs::create_dir(&repo)?;
        let device = Device::open(d.path().join("state"), None)?;
        let share = ShareStore::register(device.root().into(), &repo)?;
        let host = Arc::new(Host::new(device, share, Arc::new(|_| {})));
        let (tx, rx) = watch::channel(Shutdown::Running);
        let running = tokio::spawn(run(Empty, host, rx));
        tx.send(Shutdown::Drain)?;
        tokio::time::timeout(std::time::Duration::from_secs(1), running).await???;
        Ok(())
    }
}
