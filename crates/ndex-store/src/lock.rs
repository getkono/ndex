//! Advisory write lock and filesystem detection (PRD §11.3).
//!
//! Implemented with `rustix` (`flock` for the lock; `statfs` for NFS detection); the
//! rotational check reads `/sys/block/<dev>/queue/rotational` (PRD §6.2).

use std::fs::File;
use std::path::Path;

use ndex_core::error::Result;

/// A held exclusive advisory lock on `.ndex/lock`. Dropping the `File` releases it.
///
/// A single `IndexLock` guards writes to *both* SQLite databases (PRD §11.3).
pub struct IndexLock {
    _file: File,
}

impl IndexLock {
    /// Acquire the exclusive write lock, blocking until available.
    pub fn acquire(ndex_dir: &Path) -> Result<Self> {
        // TODO(skeleton): open <ndex_dir>/lock and rustix::fs::flock(LockExclusive).
        let _ = ndex_dir;
        todo!()
    }

    /// Try to acquire without blocking; `Ok(None)` if another writer holds it (PRD §6.2, LOCK_NB).
    pub fn try_acquire(ndex_dir: &Path) -> Result<Option<Self>> {
        let _ = ndex_dir;
        todo!()
    }
}

/// Detect whether `path` lives on an NFS mount (via `statfs` `f_type`); the caller aborts
/// because `flock()` cannot guarantee exclusion on NFS (PRD §11.3).
pub fn detect_nfs(path: &Path) -> Result<bool> {
    let _ = path;
    todo!()
}

/// Detect rotational storage via `/sys/block/<dev>/queue/rotational` (PRD §6.2).
pub fn is_rotational(path: &Path) -> Result<bool> {
    let _ = path;
    todo!()
}
