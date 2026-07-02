//! Compile-time constants shared across ndex.

/// Magic preamble written by `ndex-remote serve` before any framing (PRD §12.2).
/// 6 bytes: ASCII `NDEX` + null byte + protocol-epoch byte `0x01`.
pub const MAGIC_PREAMBLE: &[u8] = b"NDEX\x00\x01";

/// Maximum size of a single IPC frame payload, in bytes (PRD §12.2).
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Maximum stdout garbage the client scans through before giving up on the preamble (PRD §12.2).
pub const MAX_PREAMBLE_SCAN_BYTES: usize = 4096;

/// Name of the index directory placed at the archive root.
pub const NDEX_DIR: &str = ".ndex";

/// Name of the staging directory used during `reindex` (PRD §5.3).
pub const NDEX_OLD_DIR: &str = ".ndex.old";

/// Advisory write-lock file within `.ndex/`.
pub const LOCK_FILE: &str = "lock";
/// Index identity file (never modified after `init`).
pub const INDEX_TOML: &str = "index.toml";
/// User-editable settings file.
pub const CONFIG_TOML: &str = "config.toml";
/// SQLite manifest database.
pub const MANIFEST_DB: &str = "manifest.db";
/// SQLite metadata database.
pub const META_DB: &str = "meta.db";
/// Tantivy full-text index directory.
pub const CONTENT_DIR: &str = "content";
/// usearch vector index directory.
pub const VECTORS_DIR: &str = "vectors";

/// Query-time embedding prefix for the asymmetric arctic model (PRD §4.7).
pub const QUERY_PREFIX: &str = "query: ";

/// Default embedding model shortname (PRD §7.4).
pub const DEFAULT_MODEL: &str = "snowflake-arctic-embed-m-v2.0";

/// Filename used for `.ndexignore` ignore files (PRD §11.1).
pub const NDEXIGNORE_FILE: &str = ".ndexignore";
