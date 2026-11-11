use crate::{
    client,
    connection_code::ConnectionCode,
    endpoint::{Channel, Contact},
    git,
    model::Peer,
    platform,
    protocol::{Code, EnrollmentNotice, Notify},
    server::{self, Host, Shutdown},
    storage::{Alias, Device, FileLock, ShareStore},
};
use anstyle::{AnsiColor, Effects, Style};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::watch;

const BOLD: Style = Style::new().effects(Effects::BOLD);
const HIGHLIGHT: Style = AnsiColor::Cyan.on_default().effects(Effects::BOLD);
const ATTENTION: Style = AnsiColor::Yellow.on_default();
const SUCCESS: Style = AnsiColor::Green.on_default();

#[derive(Parser)]
#[command(
    version,
    about = "Share one Git repository securely over offline Bluetooth"
)]
pub struct Cli {
    /// User state directory (Git helper uses BLUGIT_HOME)
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Share an existing bare repository until Ctrl+C
    Host {
        repository: PathBuf,
        /// RFCOMM channel 1..30; omit to allocate a free channel automatically
        #[arg(long)]
        channel: Option<Channel>,
    },
    /// Register with a host's connection code, then enter its six-digit confirmation
    Join {
        code: ConnectionCode,
        /// Local name used by git clone blugit://NAME
        name: Alias,
    },
    /// Show this device's identity, or change its display name
    Device {
        #[arg(long)]
        name: Option<String>,
    },
    /// List devices allowed to access this repository
    Peers { repository: PathBuf },
    /// Revoke a device's access; already-started transfers may finish
    Revoke {
        repository: PathBuf,
        device_id: String,
    },
    #[cfg(feature = "test-transport")]
    #[command(hide = true, name = "__test-host")]
    TestHost { repository: PathBuf },
}

fn notice() -> Notify {
    Arc::new(|n: EnrollmentNotice| {
        anstream::eprintln!(
            "\n{ATTENTION}!{ATTENTION:#} {BOLD}Registration requested{BOLD:#} from {:?} [{}]\
             \n  Six-digit code: {HIGHLIGHT}{}{HIGHLIGHT:#}\
             \n  One attempt. Expires in 180 seconds.",
            n.peer.name,
            n.peer.short_id(),
            n.code.as_str()
        )
    })
}

fn suggested_alias(repository: &Path) -> Alias {
    let name = repository
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let stem = match name.rsplit_once('.') {
        Some((stem, extension)) if extension.eq_ignore_ascii_case("git") => stem,
        _ => &name,
    };
    let alias = stem
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let alias = alias.trim_start_matches(['-', '_']);
    alias[..alias.len().min(32)]
        .parse()
        .unwrap_or_else(|_| "repo".parse().expect("valid fallback alias"))
}

fn print_host(repository: &Path, peer: &Peer, code: ConnectionCode, automatic: bool) {
    let name = suggested_alias(repository);
    let channel = code.0.channel.get();
    let allocation = if automatic { "automatic" } else { "specified" };
    anstream::println!(
        "{BOLD}Sharing{BOLD:#} {}\
         \n  Device:          {} [{}]\
         \n  Channel:         {channel} ({allocation})\
         \n  Connection code: {HIGHLIGHT}{code}{HIGHLIGHT:#}\
         \n\nOn the guest, run (you can change the local name):\
         \n  {BOLD}blugit join {code} {name}{BOLD:#}",
        repository.display(),
        peer.name,
        peer.short_id(),
    );
    anstream::eprintln!(
        "\nPress {BOLD}Ctrl+C{BOLD:#} to stop sharing. Active transfers may finish."
    );
}

fn root(path: Option<PathBuf>) -> Result<PathBuf> {
    match path {
        Some(p) => Ok(p),
        None => Device::default_root(),
    }
}
fn signals() -> (watch::Receiver<Shutdown>, tokio::task::JoinHandle<()>) {
    let (tx, rx) = watch::channel(Shutdown::Running);
    let task = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = tx.send(Shutdown::Drain);
        }
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = tx.send(Shutdown::Abort);
        }
        std::future::pending::<()>().await;
    });
    (rx, task)
}
pub async fn run() -> Result<u8> {
    let cli = Cli::parse();
    let root = root(cli.state_dir)?;
    match cli.command {
        Command::Host {
            repository,
            channel,
        } => {
            let repository = git::validate_repository(&repository).await?;
            let device = Device::open(root, None)?;
            let _lock = FileLock::acquire(&device.root().join("hosting.lock"), false)?;
            let share = ShareStore::register(device.root().into(), &repository)?;
            let (mut rx, signals) = signals();
            let listener = tokio::select! {
                result=platform::listen(channel)=>result?,
                _=rx.changed()=>{signals.abort();return Ok(130);},
            };
            let code = ConnectionCode(Contact {
                address: listener.address,
                channel: Channel::try_from(listener.channel)?,
            });
            print_host(&repository, device.peer(), code, channel.is_none());
            let result =
                server::run(listener, Arc::new(Host::new(device, share, notice())), rx).await;
            signals.abort();
            result?;
        }
        #[cfg(feature = "test-transport")]
        Command::TestHost { repository } => {
            use async_trait::async_trait;
            struct Tcp(tokio::net::TcpListener);
            #[async_trait]
            impl platform::Acceptor for Tcp {
                async fn accept(&mut self) -> Result<platform::Connection> {
                    Ok(Box::new(self.0.accept().await?.0))
                }
            }
            let repository = git::validate_repository(&repository).await?;
            let device = Device::open(root, None)?;
            let _lock = FileLock::acquire(&device.root().join("hosting.lock"), false)?;
            let share = ShareStore::register(device.root().into(), &repository)?;
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let code = ConnectionCode(Contact {
                address: "AA:BB:CC:DD:EE:FF".parse()?,
                channel: Channel::try_from(7)?,
            });
            println!(
                "TEST_ADDR={}\nCONNECTION_CODE={code}",
                listener.local_addr()?
            );
            let (rx, signals) = signals();
            let result = server::run(
                Tcp(listener),
                Arc::new(Host::new(device, share, notice())),
                rx,
            )
            .await;
            signals.abort();
            result?;
        }
        command => {
            tokio::select! {
                result=execute(command,root)=>result?,
                _=tokio::signal::ctrl_c()=>return Ok(130),
            }
        }
    }
    Ok(0)
}
async fn execute(command: Command, root: PathBuf) -> Result<()> {
    match command {
        Command::Device { name } => {
            let d = Device::open(root, name)?;
            println!(
                "Device: {}\nID: {}\nState: {}",
                d.peer().name,
                d.peer().id(),
                d.root().display()
            );
        }
        Command::Join { code, name } => {
            let device = Device::open(root, None)?;
            let host = client::enroll(&device, code.0, &name, |host| async move {
                anstream::eprintln!(
                    "{BOLD}Registration requested{BOLD:#} from {:?}.\n\n\
                     Ask the host operator for the code.",
                    host.name
                );
                anstream::eprint!("{BOLD}Six-digit code:{BOLD:#} ");
                std::io::stderr().flush()?;
                let (tx, rx) = tokio::sync::oneshot::channel();
                std::thread::spawn(move || {
                    let mut input = String::new();
                    let result = std::io::stdin()
                        .read_line(&mut input)
                        .map(|_| input.trim().to_owned());
                    let _ = tx.send(result);
                });
                Code::parse(&rx.await.context("code reader stopped")??)
            })
            .await?;
            anstream::println!(
                "\n{SUCCESS}✓{SUCCESS:#} Registered with {} [{}] as {BOLD}{name}{BOLD:#}.\
                 \n\nTo clone the repository:\
                 \n  {BOLD}git clone {}{BOLD:#}",
                host.name,
                host.short_id(),
                name.url()
            );
        }
        Command::Peers { repository } => {
            let share = ShareStore::load(root, &repository)?;
            let peers = share.members()?;
            if peers.is_empty() {
                println!("No registered devices.");
            }
            for peer in peers {
                println!("{}\t{}", peer.id(), peer.name);
            }
        }
        Command::Revoke {
            repository,
            device_id,
        } => {
            let share = ShareStore::load(root, &repository)?;
            let peer = share.revoke(&device_id)?;
            println!(
                "Revoked {} [{}]. New connections will be refused.",
                peer.name,
                peer.short_id()
            );
        }
        _ => unreachable!("hosting is handled separately"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_a_valid_join_name_from_the_repository() {
        for (repository, expected) in [
            ("../git-test/hoge.git/", "hoge"),
            ("test.git", "test"),
            ("My Project.GIT", "my-project"),
            ("Repo_v2", "repo_v2"),
            ("__Test.Tools.git", "test-tools"),
            ("日本語.git", "repo"),
            (".git", "repo"),
            (
                "abcdefghijklmnopqrstuvwxyz0123456789.git",
                "abcdefghijklmnopqrstuvwxyz012345",
            ),
        ] {
            assert_eq!(suggested_alias(Path::new(repository)).to_string(), expected);
        }
    }

    #[test]
    fn cli_exposes_only_v1_operations() {
        let automatic = Cli::try_parse_from(["blugit", "host", "demo.git"]).unwrap();
        assert!(matches!(
            automatic.command,
            Command::Host { channel: None, .. }
        ));
        let specified =
            Cli::try_parse_from(["blugit", "host", "demo.git", "--channel", "8"]).unwrap();
        assert!(
            matches!(specified.command, Command::Host { channel: Some(c), .. } if c.get() == 8)
        );
        assert!(Cli::try_parse_from(["blugit", "host", "demo.git", "--channel", "0"]).is_err());
        for command in ["spike", "discover", "devices", "team"] {
            assert!(Cli::try_parse_from(["blugit", command]).is_err());
        }
        let code = ConnectionCode(Contact {
            address: "00:A7:50:41:8D:47".parse().unwrap(),
            channel: Channel::try_from(7).unwrap(),
        })
        .to_string();
        assert!(Cli::try_parse_from(["blugit", "join", &code, "demo"]).is_ok());
        assert!(Cli::try_parse_from(["blugit", "join", &code]).is_err());
        assert!(Cli::try_parse_from(["blugit", "join", &code, "../bad"]).is_err());
        assert!(Cli::try_parse_from(["blugit", "join", "123456", "demo"]).is_err());
        assert!(Cli::try_parse_from(["blugit", "join", &code, "--as", "demo"]).is_err());
    }
}
