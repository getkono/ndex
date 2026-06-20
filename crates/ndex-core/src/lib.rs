//! `ndex-core` — the foundation crate.
//!
//! Shared types, errors, configuration, raw-bytes paths, domain model, and progress
//! reporting used by every other ndex crate. It has no internal dependencies, sitting at
//! the bottom of the workspace dependency graph (PRD §3).

pub mod config;
pub mod constants;
pub mod error;
pub mod filters;
pub mod identity;
pub mod model;
pub mod path;
pub mod progress;
pub mod status;
pub mod tokens;

pub use config::{ByteSize, Config, DurationSetting};
pub use error::{NdexError, Result};
pub use filters::{SearchFilters, SearchMode};
pub use identity::{IndexIdentity, SCHEMA_VERSION};
pub use model::{
    ArchiveMeta, Blake3, Block, BlockType, Chunk, DirWalkEntry, DocMeta, Embedding, FileRecord,
    MediaMeta, ProcessedFile, WalkEntry,
};
pub use path::NdexPath;
pub use progress::{NullSink, ProgressChildUpdate, ProgressKind, ProgressSink, ProgressUpdate};
pub use status::{FileStatus, InvalidFileStatus};
pub use tokens::TokenCounter;
