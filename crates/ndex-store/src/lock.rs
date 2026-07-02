//! Advisory write lock and filesystem detection (PRD §11.3).
//!
//! Implemented with `rustix` (`flock` for the lock; `statfs` for NFS detection); the
//! rotational check reads `/sys/block/<dev>/queue/rotational` (PRD §6.2).

use std::fs::{File, OpenOptions};
use std::path::Path;

use ndex_core::constants::LOCK_FILE;
use ndex_core::error::{NdexError, Result};
use rustix::fs::{FlockOperation, flock};

/// A held exclusive advisory lock on `.ndex/lock`. Dropping the `File` releases it.
///
/// A single `IndexLock` guards writes to *both* SQLite databases (PRD §11.3).
pub struct IndexLock {
    _file: File,
}

impl IndexLock {
    /// Acquire the exclusive write lock, blocking until available.
    pub fn acquire(ndex_dir: &Path) -> Result<Self> {
        let file = open_lock_file(ndex_dir)?;
        flock(&file, FlockOperation::LockExclusive)
            .map_err(|e| NdexError::Lock(format!("failed to acquire write lock: {e}")))?;
        Ok(Self { _file: file })
    }

    /// Try to acquire without blocking; `Ok(None)` if another writer holds it (PRD §6.2, LOCK_NB).
    pub fn try_acquire(ndex_dir: &Path) -> Result<Option<Self>> {
        let file = open_lock_file(ndex_dir)?;
        match flock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(e) if e == rustix::io::Errno::WOULDBLOCK => Ok(None),
            Err(e) => Err(NdexError::Lock(format!(
                "failed to try-acquire write lock: {e}"
            ))),
        }
    }
}

/// Open (creating if needed) the `.ndex/lock` file for flocking.
fn open_lock_file(ndex_dir: &Path) -> Result<File> {
    let path = ndex_dir.join(LOCK_FILE);
    Ok(OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)?)
}

/// Detect whether `path` lives on an NFS mount (via `statfs` `f_type`); the caller aborts
/// because `flock()` cannot guarantee exclusion on NFS (PRD §11.3).
pub fn detect_nfs(path: &Path) -> Result<bool> {
    let st = rustix::fs::statfs(path).map_err(std::io::Error::from)?;
    // Linux `NFS_SUPER_MAGIC`; the literal infers `f_type`'s platform integer type.
    Ok(st.f_type == 0x6969)
}

/// Detect rotational storage via `/sys/block/<dev>/queue/rotational` (PRD §6.2).
///
/// Best-effort: maps the path's device to its sysfs queue and reads the flag, defaulting to
/// `false` (treat as SSD) when the information is unavailable.
pub fn is_rotational(path: &Path) -> Result<bool> {
    let st = rustix::fs::stat(path).map_err(std::io::Error::from)?;
    let dev = st.st_dev;
    let (major, minor) = (rustix::fs::major(dev), rustix::fs::minor(dev));

    // `/sys/dev/block/<maj>:<min>/queue/rotational` for whole disks; for partitions the queue
    // lives on the parent, so fall back to that directory's `..`.
    for candidate in [
        format!("/sys/dev/block/{major}:{minor}/queue/rotational"),
        format!("/sys/dev/block/{major}:{minor}/../queue/rotational"),
    ] {
        if let Ok(contents) = std::fs::read_to_string(&candidate) {
            return Ok(contents.trim() == "1");
        }
    }
    Ok(false)
}
