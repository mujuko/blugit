//! Single-repository sharing. All production connections are authenticated.
mod cli;
pub mod client;
pub mod connection_code;
pub mod endpoint;
pub mod git;
mod helper;
pub mod model;
pub mod platform;
pub mod protocol;
mod resolver;
pub mod server;
pub mod ssh;
pub mod storage;
#[cfg(test)]
mod test_support;

pub fn entry(helper: bool) -> std::process::ExitCode {
    #[cfg(windows)]
    if let Some(code) = platform::internal_discovery_helper() {
        return std::process::ExitCode::from(code);
    }
    #[cfg(target_os = "macos")]
    if let Some(code) = platform::internal_helper() {
        return std::process::ExitCode::from(code);
    }
    let result = tokio::runtime::Runtime::new()
        .map_err(anyhow::Error::from)
        .and_then(|runtime| {
            let result = runtime.block_on(async {
                if helper {
                    tokio::select! {r=helper::run()=>r,_=tokio::signal::ctrl_c()=>Ok(130)}
                } else {
                    cli::run().await
                }
            });
            runtime.shutdown_timeout(std::time::Duration::from_secs(1));
            result
        });
    match result {
        Ok(code) => std::process::ExitCode::from(code),
        Err(error) => {
            eprintln!("blugit: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
