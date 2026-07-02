//! `ndex` thin-client command-line interface (PRD §13.1–§13.10).

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// ndex — deep file indexer for archival storage.
#[derive(Debug, Parser)]
#[command(
    name = "ndex",
    version,
    about = "deep file indexer for archival storage"
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalOpts,

    #[command(subcommand)]
    pub command: Command,
}

/// Options available on every subcommand.
#[derive(Debug, clap::Args)]
pub struct GlobalOpts {
    /// Increase verbosity (repeatable: -v, -vv, -vvv).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
    /// Suppress non-essential output.
    #[arg(short, long, global = true)]
    pub quiet: bool,
    /// When to colorize: auto | always | never.
    #[arg(long, global = true, value_name = "WHEN", default_value = "auto")]
    pub color: String,
    /// Disable OSC 8 hyperlinks.
    #[arg(long, global = true)]
    pub no_hyperlinks: bool,
    /// Override the client config file.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    // --- SSH transport (PRD §13.2) ---
    /// SSH private key.
    #[arg(long, global = true, value_name = "PATH")]
    pub ssh_key: Option<PathBuf>,
    /// SSH port.
    #[arg(long, global = true, value_name = "PORT")]
    pub ssh_port: Option<u16>,
    /// SSH username [default: $USER].
    #[arg(long, global = true, value_name = "USER")]
    pub ssh_user: Option<String>,
    /// Pass-through SSH option (repeatable).
    #[arg(long, global = true, value_name = "OPT")]
    pub ssh_option: Vec<String>,
    /// `ndex-remote` path on the server.
    #[arg(long, global = true, value_name = "PATH")]
    pub remote_path: Option<String>,
}

// `Search` carries many optional filter fields; the size gap is accepted because the CLI enum is
// constructed exactly once at parse time (boxing would also fight clap-derive's `Args` bounds).
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Search an index.
    Search(SearchArgs),
    /// Build or update the index.
    Index(IndexArgs),
    /// Initialize a new index (local-only in v0.1).
    Init(InitArgs),
    /// Show metadata for a file.
    Info(InfoArgs),
    /// Index statistics.
    Stats(TargetArg),
    /// Verify file integrity.
    Verify(VerifyArgs),
    /// Rebuild the index from scratch.
    Reindex(ReindexArgs),
    /// Remove files from the index.
    Delete(DeleteArgs),
    /// View configuration.
    Config(ConfigArgs),
    /// Generate shell completions.
    Completions(CompletionsArgs),
    /// Manage tags (planned for v0.2).
    Tag,
    /// Find duplicate files (planned for v0.2).
    Dedup,
    /// Optimize index storage (planned for v0.2).
    Compact,
}

/// A bare `[HOST:]PATH` target.
#[derive(Debug, Parser)]
pub struct TargetArg {
    /// Local path or `host:path`.
    pub target: String,
    /// Output format: pretty | json.
    #[arg(short, long, default_value = "pretty")]
    pub format: String,
}

#[derive(Debug, Parser)]
pub struct SearchArgs {
    /// Local path or `host:path`.
    pub target: String,
    /// Search query (FTS syntax or natural language).
    pub query: String,
    /// auto | fts | semantic | hybrid.
    #[arg(short, long, default_value = "auto")]
    pub mode: String,
    /// MIME filter glob (e.g. `image/*`).
    #[arg(long)]
    pub mime: Option<String>,
    /// Modified after (ISO 8601 or relative like `2w`).
    #[arg(long)]
    pub after: Option<String>,
    /// Modified before.
    #[arg(long)]
    pub before: Option<String>,
    /// Minimum size (e.g. `10MB`).
    #[arg(long)]
    pub larger: Option<String>,
    /// Maximum size.
    #[arg(long)]
    pub smaller: Option<String>,
    /// Path glob (e.g. `invoices/**/*.pdf`).
    #[arg(long)]
    pub path: Vec<String>,
    /// Tag filter (repeatable, OR).
    #[arg(long)]
    pub tag: Vec<String>,
    /// Language filter (ISO 639-1).
    #[arg(long)]
    pub lang: Option<String>,
    /// Max results.
    #[arg(short = 'n', long, default_value_t = 20)]
    pub limit: u32,
    /// Pagination offset.
    #[arg(long, default_value_t = 0)]
    pub offset: u32,
    /// pretty | plain | json | jsonl | paths | csv.
    #[arg(short, long, default_value = "pretty")]
    pub format: String,
    /// Show scoring breakdown.
    #[arg(long)]
    pub explain: bool,
    /// Skip auto-refresh; search the stale index.
    #[arg(long)]
    pub no_refresh: bool,
    /// Force a refresh even if fresh.
    #[arg(long)]
    pub refresh: bool,
    /// Exit with code 7 if there are no results.
    #[arg(long)]
    pub fail_no_results: bool,
}

#[derive(Debug, Parser)]
pub struct IndexArgs {
    pub target: String,
    #[arg(long)]
    pub full: bool,
    #[arg(long)]
    pub verify: bool,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub jobs: Option<u32>,
    #[arg(long)]
    pub no_vectors: bool,
    #[arg(long)]
    pub only_new: bool,
}

#[derive(Debug, Parser)]
pub struct InitArgs {
    /// Local path (remote init is planned for v0.2; PRD §13.4).
    pub path: PathBuf,
    #[arg(long, default_value = "default")]
    pub model: String,
    #[arg(long)]
    pub exclude: Vec<String>,
    #[arg(long)]
    pub no_fts: bool,
    #[arg(long)]
    pub no_meta: bool,
}

#[derive(Debug, Parser)]
pub struct InfoArgs {
    pub target: String,
    pub file: String,
    #[arg(short, long, default_value = "pretty")]
    pub format: String,
}

#[derive(Debug, Parser)]
pub struct VerifyArgs {
    pub target: String,
    #[arg(long)]
    pub sample: Option<f64>,
    #[arg(long)]
    pub path: Vec<String>,
    #[arg(long)]
    pub fail_fast: bool,
}

#[derive(Debug, Parser)]
pub struct ReindexArgs {
    pub target: String,
    #[arg(long)]
    pub vectors: bool,
    #[arg(long)]
    pub fts: bool,
    #[arg(long)]
    pub all: bool,
    #[arg(long)]
    pub confirm: bool,
}

#[derive(Debug, Parser)]
pub struct DeleteArgs {
    pub target: String,
    pub glob: String,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub confirm: bool,
}

#[derive(Debug, Parser)]
pub struct ConfigArgs {
    pub target: String,
    pub key: Option<String>,
}

#[derive(Debug, Parser)]
pub struct CompletionsArgs {
    pub shell: clap_complete::Shell,
}
