use crate::{
    endpoint::Endpoint,
    git,
    identity::State,
    platform,
    security::{self, EnrollmentGate, Notify},
};
use anyhow::{Context, Result, ensure};
use clap::{Args, Subcommand};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[derive(Args)]
pub struct StateArgs {
    /// Spike identity directory (Git helper uses BLUGIT_STATE_DIR)
    #[arg(long)]
    state_dir: Option<PathBuf>,
    /// Persistent display name; identity is the automatically generated key
    #[arg(long)]
    name: Option<String>,
}

impl StateArgs {
    fn open(self) -> Result<State> {
        State::open(
            match self.state_dir {
                Some(root) => root,
                None => State::default_root()?,
            },
            self.name,
        )
    }
}

#[derive(Subcommand)]
pub enum Commands {
    /// Serve a single fixed test team/repository with code enrollment and encrypted Git
    Host {
        repository: PathBuf,
        #[command(flatten)]
        state: StateArgs,
        /// Omit the fixed test team from discovery, but accept direct enrollment
        #[arg(long)]
        hidden: bool,
    },
    /// Request a new code on the host, then type the six digits at the prompt
    Join {
        url: Endpoint,
        #[command(flatten)]
        state: StateArgs,
    },
    /// Show or rename this device's spike identity
    Identity {
        #[command(flatten)]
        state: StateArgs,
    },
    /// Actively inquire for nearby Classic Bluetooth devices and probe blugit hosts
    Discover {
        #[arg(long, default_value_t=8, value_parser=clap::value_parser!(u8).range(2..=30))]
        seconds: u8,
        /// Print probe errors to distinguish connection failures from hidden teams
        #[arg(long)]
        verbose: bool,
    },
    /// Probe a known host directly, without nearby inquiry or enrollment
    Probe { url: Endpoint },
    #[cfg(feature = "test-transport")]
    #[command(hide = true, name = "__test-host")]
    TestHost {
        repository: PathBuf,
        #[command(flatten)]
        state: StateArgs,
        #[arg(long)]
        hidden: bool,
    },
}

fn notifier() -> Notify {
    Arc::new(|notice| {
        eprintln!(
            "JOIN team={} device={:?} id={} CODE={} (one attempt, expires in 180s)",
            security::TEAM,
            notice.peer.name,
            notice.peer.short_id(),
            notice.code
        );
    })
}

pub async fn run(command: Commands) -> Result<()> {
    match command {
        Commands::Identity { state } => {
            let state = state.open()?;
            println!(
                "Device: {} [{}]\nState: {}",
                state.peer().name,
                state.peer().short_id(),
                state.root().display()
            );
        }
        Commands::Join { url, state } => {
            ensure!(
                url.secure,
                "use the host's security-spike URL ending in ?secure=1"
            );
            let state = state.open()?;
            let stream = platform::connect(&url).await?;
            let host = security::enroll(stream, &state, url.address, |host| async move {
                eprintln!(
                    "Enrollment requested. Ask the host operator for the code ({:?}).",
                    host.name
                );
                use std::io::Write;
                eprint!("Six-digit code: ");
                std::io::stderr().flush()?;
                // An ordinary stdin reader keeps Ctrl+C/expiry responsive without
                // leaving the terminal in no-echo mode when the task is cancelled.
                let (tx, rx) = tokio::sync::oneshot::channel();
                std::thread::spawn(move || {
                    let mut input = String::new();
                    let result = std::io::stdin()
                        .read_line(&mut input)
                        .map(|_| input.trim().to_owned());
                    let _ = tx.send(result);
                });
                Ok(rx.await.context("code input reader stopped")??)
            })
            .await?;
            println!(
                "Joined {} on {} [{}]. Future Git connections authenticate automatically.",
                security::TEAM,
                host.name,
                host.short_id()
            );
        }
        Commands::Host {
            repository,
            state,
            hidden,
        } => {
            let repository = git::validate_repository(&repository).await?;
            let state = state.open()?;
            let mut listener = platform::listen().await?;
            println!(
                "Security spike: {} [{}], fixed team={}, advertised={}",
                state.peer().name,
                state.peer().short_id(),
                security::TEAM,
                !hidden
            );
            println!("State: {}", state.root().display());
            println!(
                "Join: blugit spike join '{}'",
                Endpoint::secure_url(listener.address, None)
            );
            println!(
                "Clone after joining: git clone '{}'",
                Endpoint::secure_url(listener.address, None)
            );
            println!(
                "Direct channel URL: {}",
                Endpoint::secure_url(listener.address, Some(listener.channel))
            );
            eprintln!(
                "Nearby discovery also requires this PC to be discoverable in OS Bluetooth settings."
            );
            let server = Arc::new(Server::new(repository, state, hidden));
            loop {
                server.spawn(listener.accept().await?);
            }
        }
        Commands::Probe { url } => {
            ensure!(url.secure, "use the security-spike URL ending in ?secure=1");
            let advertisement = security::discover(platform::connect(&url).await?).await?;
            match (advertisement.team, advertisement.host) {
                (Some(team), Some(host)) => println!("{team}\t{} [{}]", host.name, host.short_id()),
                (None, None) => println!("Host reachable; test team is hidden."),
                _ => anyhow::bail!("inconsistent discovery response"),
            }
            eprintln!("Discovery data is unverified until code enrollment.");
        }
        Commands::Discover { seconds, verbose } => {
            eprintln!("Inquiring for nearby Bluetooth Classic devices ({seconds}s)...");
            let devices = platform::nearby(Duration::from_secs(seconds.into())).await?;
            eprintln!(
                "Probing {} candidate device(s), at most 5s each...",
                devices.len().min(32)
            );
            let mut found = 0;
            for (address, _) in devices.into_iter().take(32) {
                let endpoint = Endpoint {
                    address,
                    channel: None,
                    secure: true,
                };
                let probe = async { security::discover(platform::connect(&endpoint).await?).await };
                match tokio::time::timeout(Duration::from_secs(5), probe).await {
                    Ok(Ok(ad)) if ad.team.is_some() => {
                        if let Some(host) = ad.host {
                            println!(
                                "{}\t{} [{}]\t{}",
                                ad.team.unwrap(),
                                host.name,
                                host.short_id(),
                                Endpoint::secure_url(address, None)
                            );
                            found += 1;
                        }
                    }
                    Ok(Ok(_)) => {
                        if verbose {
                            eprintln!("{address}: no advertised team");
                        }
                    }
                    Ok(Err(error)) => {
                        if verbose {
                            eprintln!("{address}: {error:#}");
                        }
                    }
                    Err(_) => {
                        if verbose {
                            eprintln!("{address}: probe timed out");
                        }
                    }
                }
            }
            eprintln!(
                "Found {found} advertised test team(s). Discovery names/IDs are unverified until enrollment."
            );
        }
        #[cfg(feature = "test-transport")]
        Commands::TestHost {
            repository,
            state,
            hidden,
        } => {
            let repository = git::validate_repository(&repository).await?;
            let server = Arc::new(Server::new(repository, state.open()?, hidden));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            println!("{}", listener.local_addr()?);
            loop {
                server.spawn(Box::new(listener.accept().await?.0));
            }
        }
    }
    Ok(())
}

struct Server {
    repository: PathBuf,
    state: State,
    hidden: bool,
    gate: EnrollmentGate,
    notify: Notify,
    slots: Arc<tokio::sync::Semaphore>,
}

impl Server {
    fn new(repository: PathBuf, state: State, hidden: bool) -> Self {
        Self {
            repository,
            state,
            hidden,
            gate: EnrollmentGate::default(),
            notify: notifier(),
            slots: Arc::new(tokio::sync::Semaphore::new(16)),
        }
    }

    fn spawn(self: &Arc<Self>, stream: platform::Connection) {
        let Ok(permit) = self.slots.clone().try_acquire_owned() else {
            eprintln!("Spike connection limit reached");
            return;
        };
        let server = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let result = async {
                if let Some(stream) = security::accept(
                    stream,
                    &server.state,
                    server.hidden,
                    &server.gate,
                    &server.notify,
                )
                .await?
                {
                    git::serve(stream, &server.repository)
                        .await
                        .context("authenticated Git transfer")?;
                }
                Ok::<(), anyhow::Error>(())
            }
            .await;
            if let Err(error) = result {
                eprintln!("Spike connection ended: {error:#}");
            }
        });
    }
}
