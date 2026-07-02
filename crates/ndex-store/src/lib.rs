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

pub use fts::{FtsFileMeta, FtsHit, FtsIndex};
pub use lock::IndexLock;
pub use manifest::{Change, Manifest, RunKind};
pub use meta::MetaDb;
pub use vector::{Sidecar, SidecarEntry, VecHit, VectorIndex};

/// All index engines for a single `.ndex/` root, opened together under the index lock
/// (exclusive for writers via [`Store::open`]/[`Store::create`], shared for readers via
/// [`Store::open_read`]; PRD §10.6).
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

/// How a `Store::open*` call locks `.ndex/lock` (see [`IndexLock`]).
enum LockMode {
    Exclusive,
    /// Non-blocking exclusive: `open_with` returns `Ok(None)` when the lock is held.
    TryExclusive,
    Shared,
}

impl Store {
    /// Open an existing index at `<root>/.ndex/` for **writing** (verifies identity,
    /// acquires the blocking **exclusive** lock — see [`IndexLock::acquire`]).
    ///
    /// Readers should use [`Store::open_read`] instead; exclusive opens serialize behind
    /// each other and behind readers. Callers that must not block should use
    /// [`Store::try_open`].
    ///
    /// The vector index is not yet loaded in v0.1 (semantic retrieval is a follow-up); searches
    /// fall back to FTS via the empty-vector path (PRD §16.3).
    pub fn open(root: &Path) -> Result<Self> {
        Ok(Self::open_with(root, LockMode::Exclusive)?
            .expect("blocking exclusive open always acquires the lock"))
    }

    /// Try to open an existing index at `<root>/.ndex/` for **writing** without blocking.
    ///
    /// Identical to [`Store::open`] except the lock step uses [`IndexLock::try_acquire`]
    /// (`LOCK_NB`): if another process holds the index lock, returns `Ok(None)` instead of
    /// waiting. This is the fail-fast path for interactive writers (and the PRD §6.2
    /// skip-silently path for auto-refresh).
    pub fn try_open(root: &Path) -> Result<Option<Self>> {
        Self::open_with(root, LockMode::TryExclusive)
    }

    /// Open an existing index at `<root>/.ndex/` for **reading** (verifies identity,
    /// acquires the blocking **shared** lock — see [`IndexLock::acquire_shared`]).
    ///
    /// Any number of readers coexist: SQLite WAL supports concurrent readers alongside a
    /// writer, and tantivy searches run on point-in-time snapshot readers. The call still
    /// blocks while a writer holds the exclusive lock. The FTS index is opened without a
    /// writer ([`FtsIndex::open_readonly`] — tantivy's writer lock is exclusive), so FTS
    /// write operations on a read `Store` fail with `NdexError::Index`.
    pub fn open_read(root: &Path) -> Result<Self> {
        Ok(Self::open_with(root, LockMode::Shared)?
            .expect("blocking shared open always acquires the lock"))
    }

    /// Shared body of [`Store::open`] / [`Store::try_open`] / [`Store::open_read`]; only the
    /// lock mode and the FTS open (writer vs read-only) differ. Returns `Ok(None)` only for
    /// [`LockMode::TryExclusive`] when the lock is already held.
    fn open_with(root: &Path, mode: LockMode) -> Result<Option<Self>> {
        let ndex_dir = root.join(NDEX_DIR);
        if !ndex_dir.join(INDEX_TOML).is_file() {
            return Err(NdexError::IndexNotFound(root.display().to_string()));
        }
        if lock::detect_nfs(&ndex_dir)? {
            return Err(NdexError::Nfs(ndex_dir.display().to_string()));
        }
        let lock = match mode {
            LockMode::Exclusive => IndexLock::acquire(&ndex_dir)?,
            LockMode::TryExclusive => match IndexLock::try_acquire(&ndex_dir)? {
                Some(lock) => lock,
                None => return Ok(None),
            },
            LockMode::Shared => IndexLock::acquire_shared(&ndex_dir)?,
        };
        let identity = identity::open_identity(&ndex_dir)?;
        let config_path = ndex_dir.join(CONFIG_TOML);
        let config = if config_path.is_file() {
            Config::load(&config_path)?
        } else {
            Config::default()
        };
        let manifest = Manifest::open_or_create(&ndex_dir.join(MANIFEST_DB))?;
        let meta = MetaDb::open_or_create(&ndex_dir.join(META_DB))?;
        let fts = match mode {
            LockMode::Exclusive | LockMode::TryExclusive => {
                FtsIndex::open_or_create(&ndex_dir.join(CONTENT_DIR))?
            }
            LockMode::Shared => FtsIndex::open_readonly(&ndex_dir.join(CONTENT_DIR))?,
        };
        Ok(Some(Self {
            identity,
            config,
            manifest,
            meta,
            fts,
            vectors: None,
            lock,
            root: root.to_path_buf(),
        }))
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

    /// Borrow the held lock (exclusive for `open`/`create`, shared for `open_read`;
    /// kept alive for the lifetime of the `Store`).
    pub fn lock(&self) -> &IndexLock {
        &self.lock
    }

    /// The archive root directory (the parent of `.ndex/`).
    pub fn root(&self) -> &Path {
        &self.root
    }
}
