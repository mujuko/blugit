pub mod endpoint;
pub mod git;
pub mod helper;
pub mod identity;
pub mod platform;
mod secure_stream;
pub mod security;
mod spike;
pub mod wire;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::{path::PathBuf, process::ExitCode};

pub const SERVICE: uuid::Uuid = uuid::Uuid::from_u128(0x2d508ec769a447f2a90cb54df08c3cf5);

#[derive(Parser)]
#[command(version, about = "Minimal Git over Bluetooth RFCOMM")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Publish one existing bare repository
    Host {
        repository: PathBuf,
        /// Override the address displayed to the guest; does not select a radio
        #[arg(long)]
        address: Option<endpoint::Address>,
    },
    /// Show this PC's Bluetooth address and clone URL
    Address,
    /// List paired devices (not nearby discovery)
    Devices,
    /// Stage-1 technical spike: code enrollment, authenticated Git and discovery
    Spike {
        #[command(subcommand)]
        command: spike::Commands,
    },
    #[cfg(feature = "test-transport")]
    #[command(hide = true, name = "__test-host")]
    TestHost { repository: PathBuf },
}

pub fn entry(helper: bool) -> ExitCode {
    #[cfg(target_os = "macos")]
    if let Some(code) = platform::internal_helper() {
        return ExitCode::from(code);
    }
    let result = tokio::runtime::Runtime::new()
        .context("start async runtime")
        .and_then(|runtime| {
            // stdio uses ordinary daemon threads, never Tokio's blocking stdin:
            // Git may keep stdin open until this helper process has exited.
            let result = runtime.block_on(async {
                tokio::select! {
                    result = async { if helper { helper::run().await } else { run().await } } => result,
                    _ = tokio::signal::ctrl_c() => Ok(130),
                }
            });
            // A stuck OS Bluetooth discovery call must not prevent process exit
            // after the connection timeout or Ctrl+C.
            runtime.shutdown_timeout(std::time::Duration::from_secs(1));
            result
        });
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("blugit: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<u8> {
    match Cli::parse().command {
        Commands::Spike { command } => spike::run(command).await?,
        Commands::Address => {
            let address = platform::local_address().await?;
            println!("Bluetooth address: {address}\nURL: {}", address.url(None));
        }
        Commands::Devices => {
            println!("Paired devices only; this is not nearby discovery.");
            let devices = platform::devices().await?;
            if devices.is_empty() {
                println!(
                    "No paired devices. Copy the clone URL from 'blugit host' on the host PC."
                );
            }
            for (address, name) in devices {
                println!("{name}\t{address}\t{}", address.url(None));
            }
            println!(
                "If the host is missing, run 'blugit address' on it or copy its host startup URL."
            );
        }
        Commands::Host {
            repository,
            address,
        } => {
            let repository = git::validate_repository(&repository).await?;
            let mut listener = platform::listen().await?;
            let address = address.unwrap_or(listener.address);
            println!(
                "Serving {} (RFCOMM channel {})",
                repository.display(),
                listener.channel
            );
            println!("Bluetooth address: {address}");
            println!(
                "On the guest: git -c init.defaultBranch=main clone {} demo-work",
                address.url(None)
            );
            println!(
                "Direct channel URL (skip service lookup): {}",
                address.url(Some(listener.channel))
            );
            loop {
                let connection = listener.accept().await?;
                let repository = repository.clone();
                tokio::spawn(async move {
                    if let Err(error) = git::serve(connection, &repository).await {
                        eprintln!("Connection ended: {error:#}");
                    }
                });
            }
        }
        #[cfg(feature = "test-transport")]
        Commands::TestHost { repository } => {
            let repository = git::validate_repository(&repository).await?;
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            println!("{}", listener.local_addr()?);
            loop {
                let (stream, _) = listener.accept().await?;
                let repository = repository.clone();
                tokio::spawn(async move {
                    if let Err(error) = git::serve(Box::new(stream), &repository).await {
                        eprintln!("Connection ended: {error:#}");
                    }
                });
            }
        }
    }
    Ok(0)
}
