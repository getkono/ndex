//! `ndex` — the thin client library: SSH/subprocess transport and terminal rendering.
//!
//! Parses `[HOST:]PATH` targets ([`hosts`]), spawns `ndex-remote serve` over a
//! [`Transport`](transport::Transport), drives a protocol [`Session`](session::Session), and
//! renders results ([`render`]). It depends only on `ndex-core` and `ndex-protocol` — the build
//! graph guarantees it cannot reach any engine crate (PRD §2.4).

pub mod args;
pub mod commands;
pub mod hosts;
pub mod render;
pub mod session;
pub mod transport;

use ndex_core::error::Result;

use args::{Cli, Command};

/// Initialize the tracing subscriber: logs to stderr, level from `NDEX_LOG` or `-v`/`-q` (PRD §17).
pub fn init_tracing(verbose: u8, quiet: bool) {
    use tracing_subscriber::{EnvFilter, fmt};

    let default = if quiet {
        "error"
    } else {
        match verbose {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        }
    };
    let filter = EnvFilter::try_from_env("NDEX_LOG").unwrap_or_else(|_| EnvFilter::new(default));
    let _ = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

/// Dispatch a parsed CLI to its handler (PRD §13).
pub fn run(cli: Cli) -> Result<()> {
    let Cli { global, command } = cli;
    match command {
        Command::Search(a) => commands::search(a, &global),
        Command::Index(a) => commands::index(a, &global),
        Command::Init(a) => commands::init(a, &global),
        Command::Info(a) => commands::info(a, &global),
        Command::Stats(a) => commands::stats(a, &global),
        Command::Verify(a) => commands::verify(a, &global),
        Command::Reindex(a) => commands::reindex(a, &global),
        Command::Delete(a) => commands::delete(a, &global),
        Command::Config(a) => commands::config(a, &global),
        Command::Completions(a) => commands::completions(a),
        Command::Tag => commands::unavailable_v0_2("tag"),
        Command::Dedup => commands::unavailable_v0_2("dedup"),
        Command::Compact => commands::unavailable_v0_2("compact"),
    }
}
