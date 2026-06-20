//! `ndex-remote` — the fat server library: standalone CLI dispatch + the msgpack serve loop.
//!
//! Wires the engine crates (`store`, `extract`, `embed`, `search`, `reconcile`) behind both a
//! standalone CLI ([`run`]) and the SSH serve session ([`serve`]). Translates wire types to
//! engine types at the boundary ([`map`], [`progress`]).

pub mod cli;
pub mod commands;
pub mod map;
pub mod progress;
pub mod serve;

use ndex_core::error::Result;

use cli::{Cli, Command};

/// Initialize the tracing subscriber: human-readable logs to stderr, level from `NDEX_LOG` or
/// the `-v`/`-q` flags (PRD §17).
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

/// Dispatch a parsed CLI to its handler (PRD §13.11).
pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Serve(a) => serve::serve(a),
        Command::Init(a) => commands::indexing::init(a),
        Command::Index(a) => commands::indexing::index(a),
        Command::Search(a) => commands::read::search(a),
        Command::Info(a) => commands::read::info(a),
        Command::Stats(a) => commands::read::stats(a),
        Command::Verify(a) => commands::maintain::verify(a),
        Command::Reindex(a) => commands::indexing::reindex(a),
        Command::Delete(a) => commands::maintain::delete(a),
        Command::Config(a) => commands::maintain::config(a),
        Command::Checkpoint(a) => commands::maintain::checkpoint(a),
        Command::Model(c) => commands::model::run(c),
        Command::SelfUpdate(a) => commands::model::self_update(a),
        Command::Completions(a) => commands::completions::run(a),
        Command::Tag => commands::unavailable_v0_2("tag"),
        Command::Dedup => commands::unavailable_v0_2("dedup"),
        Command::Compact => commands::unavailable_v0_2("compact"),
    }
}
