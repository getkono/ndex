//! `ndex-store` — the index engines for one `.ndex/` root.
//!
//! Wraps the SQLite manifest ([`Manifest`]) and metadata ([`MetaDb`]) databases, the
//! tantivy full-text index ([`FtsIndex`]), and the usearch vector index ([`VectorIndex`]),
//! plus the identity file and the advisory write [`IndexLock`]. Depends only on `ndex-core`.

pub mod fts;
pub mod identity;
pub mod lock;
pub mod manifest;
pub mod meta;
pub mod vector;

use std::path::{Path, PathBuf};

use ndex_core::constants::{CONFIG_TOML, CONTENT_DIR, INDEX_TOML, MANIFEST_DB, META_DB, NDEX_DIR};
use ndex_core::error::{NdexError, Result};
use ndex_core::{Config, IndexIdentity};

pub use fts::{FtsHit, FtsIndex};
pub use lock::IndexLock;
pub use manifest::{Change, Manifest, RunKind};
pub use meta::MetaDb;
pub use vector::{Sidecar, SidecarEntry, VecHit, VectorIndex};

/// All index engines for a single `.ndex/` root, opened together under the write lock (PRD §10.6).
pub struct Store {
    pub identity: IndexIdentity,
    pub config: Config,
    pub manifest: Manifest,
    pub meta: MetaDb,
    pub fts: FtsIndex,
    /// `None` when the index was created with `--model none` (PRD §13.4).
    pub vectors: Option<VectorIndex>,
    lock: IndexLock,
    root: PathBuf,
}

impl Store {
    /// Open an existing index at `<root>/.ndex/` (verifies identity, acquires the lock).
    ///
    /// The vector index is not yet loaded in v0.1 (semantic retrieval is a follow-up); searches
    /// fall back to FTS via the empty-vector path (PRD §16.3).
    pub fn open(root: &Path) -> Result<Self> {
        let ndex_dir = root.join(NDEX_DIR);
        if !ndex_dir.join(INDEX_TOML).is_file() {
            return Err(NdexError::IndexNotFound(root.display().to_string()));
        }
        if lock::detect_nfs(&ndex_dir)? {
            return Err(NdexError::Nfs(ndex_dir.display().to_string()));
        }
        let lock = IndexLock::acquire(&ndex_dir)?;
        let identity = identity::open_identity(&ndex_dir)?;
        let config_path = ndex_dir.join(CONFIG_TOML);
        let config = if config_path.is_file() {
            Config::load(&config_path)?
        } else {
            Config::default()
        };
        let manifest = Manifest::open_or_create(&ndex_dir.join(MANIFEST_DB))?;
        let meta = MetaDb::open_or_create(&ndex_dir.join(META_DB))?;
        let fts = FtsIndex::open_or_create(&ndex_dir.join(CONTENT_DIR))?;
        Ok(Self {
            identity,
            config,
            manifest,
            meta,
            fts,
            vectors: None,
            lock,
            root: root.to_path_buf(),
        })
    }

    /// Create a fresh index at `<root>/.ndex/` (PRD §13.4 `init`).
    pub fn create(root: &Path, identity: IndexIdentity, config: Config) -> Result<Self> {
        let ndex_dir = root.join(NDEX_DIR);
        if ndex_dir.join(INDEX_TOML).exists() {
            return Err(NdexError::Other(format!(
                "an index already exists at {}",
                ndex_dir.display()
            )));
        }
        std::fs::create_dir_all(&ndex_dir)?;
        if lock::detect_nfs(&ndex_dir)? {
            return Err(NdexError::Nfs(ndex_dir.display().to_string()));
        }
        let lock = IndexLock::acquire(&ndex_dir)?;
        identity::write_identity(&ndex_dir, &identity)?;
        std::fs::write(ndex_dir.join(CONFIG_TOML), config.to_toml()?)?;
        let manifest = Manifest::open_or_create(&ndex_dir.join(MANIFEST_DB))?;
        let meta = MetaDb::open_or_create(&ndex_dir.join(META_DB))?;
        let fts = FtsIndex::open_or_create(&ndex_dir.join(CONTENT_DIR))?;
        Ok(Self {
            identity,
            config,
            manifest,
            meta,
            fts,
            vectors: None,
            lock,
            root: root.to_path_buf(),
        })
    }

    /// Borrow the held write lock (kept alive for the lifetime of the `Store`).
    pub fn lock(&self) -> &IndexLock {
        &self.lock
    }

    /// The archive root directory (the parent of `.ndex/`).
    pub fn root(&self) -> &Path {
        &self.root
    }
}
