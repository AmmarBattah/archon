//! `archon`: the operator's CLI, the terminal UI launcher, the quiet chat,
//! and the entry points for `archon daemon run` and `archon mcp`
//! (ARCHITECTURE §2, §12).

mod chat;
mod cli;
mod cmd;
mod connect;
mod format;
mod instance;
mod ops;
mod questions;
mod render;
mod setup;
mod style;
mod views;
mod wrap;

use std::process::ExitCode;

use clap::Parser;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

use crate::cli::{Cli, Command, DaemonCommand};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let json = cli.json;
    // The daemon logs its work; everything else stays quiet unless RUST_LOG
    // asks. `archon mcp` speaks JSON-RPC on stdout, so logs go to stderr.
    let default_level = match &cli.command {
        Some(Command::Daemon(DaemonCommand::Run { detach: false })) => LevelFilter::INFO,
        _ => LevelFilter::WARN,
    };
    let filter = EnvFilter::builder()
        .with_default_directive(default_level.into())
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
    let result = tokio::runtime::Runtime::new()
        .map_err(anyhow::Error::from)
        .and_then(|runtime| runtime.block_on(cmd::dispatch(cli)));
    match result {
        Ok(code) => code,
        Err(error) => {
            if json {
                let code = error
                    .downcast_ref::<archon_core::error::ArchonError>()
                    .map(|e| archon_core::protocol::ErrorReply::from(e).code)
                    .unwrap_or(archon_core::protocol::ErrorCode::Internal);
                let envelope =
                    serde_json::json!({"error": {"code": code, "message": format!("{error:#}")}});
                println!("{envelope}");
            } else {
                eprintln!("archon: {error:#}");
            }
            ExitCode::FAILURE
        }
    }
}
