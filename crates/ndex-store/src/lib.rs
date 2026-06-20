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

use std::path::Path;

use ndex_core::error::Result;
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
}

impl Store {
    /// Open an existing index at `<root>/.ndex/` (verifies identity, acquires the lock).
    pub fn open(root: &Path) -> Result<Self> {
        let _ = root;
        todo!()
    }

    /// Create a fresh index at `<root>/.ndex/` (PRD §13.4 `init`).
    pub fn create(root: &Path, identity: IndexIdentity, config: Config) -> Result<Self> {
        let _ = (root, identity, config);
        todo!()
    }

    /// Borrow the held write lock (kept alive for the lifetime of the `Store`).
    pub fn lock(&self) -> &IndexLock {
        &self.lock
    }
}
