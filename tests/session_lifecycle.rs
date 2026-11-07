//! Lifecycle integration: injectable byte streams, real Git child process.
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use blugit::{
    endpoint::{Channel, Endpoint},
    platform::{Acceptor, Connection},
    protocol::{self, Code},
    server::{self, Host, Shutdown},
    storage::{Device, ShareStore},
    wire::{self, Kind},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch};

struct Queue(mpsc::Receiver<Connection>);
#[async_trait]
impl Acceptor for Queue {
    async fn accept(&mut self) -> Result<Connection> {
        self.0.recv().await.context("test acceptor closed")
    }
}

#[tokio::test]
async fn stopping_cancels_pending_registration_without_waiting_three_minutes() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo)?;
    let host = Device::open(dir.path().join("host"), Some("Host".into()))?;
    let guest = Device::open(dir.path().join("guest"), Some("Guest".into()))?;
    let share = ShareStore::register(host.root().into(), &repo)?;
    let endpoint = Endpoint {
        address: "AA:BB:CC:DD:EE:FF".parse()?,
        share: share.id(),
        channel: Channel::try_from(7)?,
    };
    let (notice_tx, mut notices) = mpsc::unbounded_channel();
    let host = Arc::new(Host::new(
        host,
        share.clone(),
        Arc::new(move |n| {
            let _ = notice_tx.send(n.code);
        }),
    ));
    let (connections, rx) = mpsc::channel(1);
    let (stop_tx, stop_rx) = watch::channel(Shutdown::Running);
    let running = tokio::spawn(server::run(Queue(rx), host, stop_rx));
    let (a, b) = tokio::io::duplex(4096);
    connections
        .send(Box::new(a))
        .await
        .map_err(|_| anyhow::anyhow!("acceptor stopped"))?;
    let client = tokio::spawn(async move {
        protocol::enroll(Box::new(b), &guest, &endpoint, |_| {
            std::future::pending::<Result<Code>>()
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(2), notices.recv())
        .await?
        .context("registration did not start")?;
    stop_tx.send(Shutdown::Drain)?;
    tokio::time::timeout(Duration::from_secs(2), running).await???;
    client.abort();
    assert!(share.members()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn graceful_stop_allows_git_to_reach_exit_ack_and_completion() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = dir.path().join("repo.git");
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
    let endpoint = Endpoint {
        address: "AA:BB:CC:DD:EE:FF".parse()?,
        share: share.id(),
        channel: Channel::try_from(7)?,
    };
    let (code_tx, mut codes) = mpsc::unbounded_channel();
    let host = Arc::new(Host::new(
        host,
        share.clone(),
        Arc::new(move |n| {
            let _ = code_tx.send(n.code);
        }),
    ));
    let (connections, rx) = mpsc::channel(2);
    let (stop_tx, stop_rx) = watch::channel(Shutdown::Running);
    let running = tokio::spawn(server::run(Queue(rx), host, stop_rx));
    let (a, b) = tokio::io::duplex(4096);
    connections
        .send(Box::new(a))
        .await
        .map_err(|_| anyhow::anyhow!("acceptor stopped"))?;
    protocol::enroll(Box::new(b), &guest, &endpoint, |_| async move {
        codes.recv().await.context("no code")
    })
    .await?;
    let (a, b) = tokio::io::duplex(65536);
    connections
        .send(Box::new(a))
        .await
        .map_err(|_| anyhow::anyhow!("acceptor stopped"))?;
    let mut client = protocol::connect(Box::new(b), &guest, &endpoint).await?;
    wire::send(&mut client, Kind::Request, b"git-upload-pack").await?;
    assert_eq!(wire::receive(&mut client).await?.0, Kind::Ready);
    stop_tx.send(Shutdown::Drain)?;
    tokio::task::yield_now().await;
    assert!(!running.is_finished(), "stop aborted a running Git session");
    wire::send(&mut client, Kind::Eof, b"").await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let (kind, _) = wire::receive(&mut client).await?;
            if kind == Kind::Exit {
                break;
            }
        }
        wire::send(&mut client, Kind::Ack, b"").await?;
        ensure!(
            wire::receive(&mut client).await? == (Kind::Complete, Vec::new()),
            "final ACK was not confirmed"
        );
        Ok::<(), anyhow::Error>(())
    })
    .await??;
    tokio::time::timeout(Duration::from_secs(2), running).await???;
    Ok(())
}
