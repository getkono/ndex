//! `ndex-remote` command-line interface (PRD §13.11, §7.4).

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// The fat server: msgpack serve loop + a standalone CLI for local/admin use.
#[derive(Debug, Parser)]
#[command(
    name = "ndex-remote",
    version,
    about = "ndex fat server (extraction, embedding, indexing, search)"
)]
pub struct Cli {
    /// Increase verbosity (repeatable: -v, -vv, -vvv).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress non-essential output.
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Write logs to a file in addition to stderr (PRD §17).
    #[arg(long, global = true, value_name = "PATH")]
    pub log_file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start a msgpack session on stdin/stdout (the command the thin client invokes over SSH).
    Serve(ServeArgs),
    /// Initialize a new index.
    Init(InitArgs),
    /// Build or update the index.
    Index(IndexArgs),
    /// Search an index.
    Search(SearchArgs),
    /// Show metadata for a file.
    Info(InfoArgs),
    /// Index statistics.
    Stats(PathArg),
    /// Verify file integrity by recomputing BLAKE3 hashes.
    Verify(VerifyArgs),
    /// Rebuild the index from scratch.
    Reindex(ReindexArgs),
    /// Remove files from the index.
    Delete(DeleteArgs),
    /// View configuration.
    Config(ConfigArgs),
    /// Fold the SQLite WAL into the main DB files for safe backup (PRD §18.3).
    Checkpoint(PathArg),
    /// Manage embedding models.
    #[command(subcommand)]
    Model(ModelCommand),
    /// Update ndex-remote in place (stub; planned for v0.2).
    SelfUpdate(SelfUpdateArgs),
    /// Generate shell completions.
    Completions(CompletionsArgs),
    /// Manage tags (planned for v0.2).
    Tag,
    /// Find duplicate files (planned for v0.2).
    Dedup,
    /// Optimize index storage (planned for v0.2).
    Compact,
}

/// A bare index-root path argument.
#[derive(Debug, Parser)]
pub struct PathArg {
    /// Index root directory.
    pub path: PathBuf,
}

#[derive(Debug, Parser)]
pub struct ServeArgs {
    /// Index root directory.
    #[arg(long)]
    pub root: PathBuf,
    /// Reject write operations (index, delete, reindex).
    #[arg(long)]
    pub read_only: bool,
    /// Exit after S seconds of inactivity (0 = no timeout).
    #[arg(long, default_value_t = 0)]
    pub timeout: u64,
}

#[derive(Debug, Parser)]
pub struct InitArgs {
    pub path: PathBuf,
    /// Embedding model: `default` (arctic) or `none`.
    #[arg(long, default_value = "default")]
    pub model: String,
    /// Gitignore-style exclude pattern (repeatable).
    #[arg(long)]
    pub exclude: Vec<String>,
    /// Disable the full-text index.
    #[arg(long)]
    pub no_fts: bool,
    /// Disable metadata extraction.
    #[arg(long)]
    pub no_meta: bool,
}

#[derive(Debug, Parser)]
pub struct IndexArgs {
    pub path: PathBuf,
    /// Force a full re-index.
    #[arg(long)]
    pub full: bool,
    /// Recompute BLAKE3 for unchanged files.
    #[arg(long)]
    pub verify: bool,
    /// Show changes without writing.
    #[arg(long)]
    pub dry_run: bool,
    /// Extraction parallelism [default: num_cpus].
    #[arg(long)]
    pub jobs: Option<u32>,
    /// Embedding batch size.
    #[arg(long)]
    pub batch_size: Option<u32>,
    /// Skip vector embedding.
    #[arg(long)]
    pub no_vectors: bool,
    /// Enable named entity recognition (accepted but ignored in v0.1).
    #[arg(long)]
    pub enable_ner: bool,
    /// Skip files above this size.
    #[arg(long)]
    pub max_file_size: Option<String>,
    /// Process only new files (skip modified).
    #[arg(long)]
    pub only_new: bool,
    /// Show current indexing status and exit.
    #[arg(long)]
    pub status: bool,
}

#[derive(Debug, Parser)]
pub struct SearchArgs {
    pub path: PathBuf,
    pub query: String,
    /// Search mode: auto | fts | semantic | hybrid.
    #[arg(short, long, default_value = "auto")]
    pub mode: String,
    /// Output format: pretty | plain | json | jsonl | paths | csv.
    #[arg(short, long, default_value = "pretty")]
    pub format: String,
    /// Max results.
    #[arg(short = 'n', long, default_value_t = 20)]
    pub limit: u32,
    /// Pagination offset.
    #[arg(long, default_value_t = 0)]
    pub offset: u32,
    /// Show scoring breakdown.
    #[arg(long)]
    pub explain: bool,
}

#[derive(Debug, Parser)]
pub struct InfoArgs {
    pub path: PathBuf,
    /// The file (within the index) to describe.
    pub file: PathBuf,
    #[arg(short, long, default_value = "pretty")]
    pub format: String,
}

#[derive(Debug, Parser)]
pub struct VerifyArgs {
    pub path: PathBuf,
    /// Verify a random sample fraction (e.g. 0.01 = 1%).
    #[arg(long)]
    pub sample: Option<f64>,
    /// Verify files matching a glob.
    #[arg(long)]
    pub path_glob: Option<String>,
    /// Stop on the first corruption.
    #[arg(long)]
    pub fail_fast: bool,
}

#[derive(Debug, Parser)]
pub struct ReindexArgs {
    pub path: PathBuf,
    /// Re-embed vectors only.
    #[arg(long)]
    pub vectors: bool,
    /// Rebuild FTS only.
    #[arg(long)]
    pub fts: bool,
    /// Full rebuild (default).
    #[arg(long)]
    pub all: bool,
    /// Skip the interactive confirmation prompt.
    #[arg(long)]
    pub confirm: bool,
}

#[derive(Debug, Parser)]
pub struct DeleteArgs {
    pub path: PathBuf,
    pub glob: String,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub confirm: bool,
}

#[derive(Debug, Parser)]
pub struct ConfigArgs {
    pub path: PathBuf,
    /// Read a single key (e.g. `auto_refresh.threshold`).
    pub key: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ModelCommand {
    /// Show available and downloaded models.
    List,
    /// Pre-download a model.
    Fetch {
        #[arg(default_value = "arctic")]
        model: String,
        /// Download all available models.
        #[arg(long)]
        all: bool,
    },
    /// Re-verify a downloaded model's integrity.
    Verify {
        #[arg(default_value = "arctic")]
        model: String,
    },
    /// Remove a downloaded model.
    Delete {
        #[arg(default_value = "arctic")]
        model: String,
    },
    /// Print the path to a model file.
    Path {
        #[arg(default_value = "arctic")]
        model: String,
    },
    /// Import a pre-staged model tarball (air-gapped servers).
    Import { tarball: PathBuf },
}

#[derive(Debug, Parser)]
pub struct SelfUpdateArgs {
    /// Update to a specific version.
    #[arg(long)]
    pub version: Option<String>,
    /// Just check, don't install.
    #[arg(long)]
    pub check: bool,
}

#[derive(Debug, Parser)]
pub struct CompletionsArgs {
    /// Target shell: bash | zsh | fish | …
    pub shell: clap_complete::Shell,
}
