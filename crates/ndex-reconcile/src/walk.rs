//! Phase 1 — filesystem walk (PRD §11.1).

use std::path::Path;

use dashmap::DashMap;
use ndex_core::config::Config;
use ndex_core::constants::{NDEX_DIR, NDEX_OLD_DIR, NDEXIGNORE_FILE};
use ndex_core::error::{NdexError, Result};
use ndex_core::model::{DirWalkEntry, WalkEntry};
use ndex_core::path::NdexPath;

/// Phase 1 output: filesystem metadata for files and directories under the root (PRD §11.1).
#[derive(Debug, Default)]
pub struct WalkOutcome {
    pub files: DashMap<NdexPath, WalkEntry>,
    pub dirs: DashMap<NdexPath, DirWalkEntry>,
}

/// Build a [`WalkEntry`] from filesystem metadata.
fn file_entry(meta: &std::fs::Metadata) -> WalkEntry {
    use std::os::unix::fs::MetadataExt;
    WalkEntry {
        size: meta.len(),
        mtime_ns: meta.mtime() * 1_000_000_000 + meta.mtime_nsec(),
        ctime_ns: meta.ctime() * 1_000_000_000 + meta.ctime_nsec(),
        inode: meta.ino(),
        dev: meta.dev(),
        mode: meta.mode(),
    }
}

fn dir_entry(meta: &std::fs::Metadata) -> DirWalkEntry {
    use std::os::unix::fs::MetadataExt;
    DirWalkEntry {
        mtime_ns: meta.mtime() * 1_000_000_000 + meta.mtime_nsec(),
        ctime_ns: meta.ctime() * 1_000_000_000 + meta.ctime_nsec(),
        inode: meta.ino(),
        dev: meta.dev(),
        mode: meta.mode(),
    }
}

/// Walk `root` via the `ignore` crate, honoring `.gitignore`/`.ndexignore`, skipping non-regular
/// files, and recording filesystem metadata for files and directories (PRD §11.1).
///
/// Symlink containment (PRD §11.4): symlinked entries whose canonicalized target escapes
/// the canonicalized root are skipped (with their subtrees), so a link to e.g. `/etc`
/// never pulls outside content into the index. Symlinks resolving *within* the root are
/// followed normally when `walk.follow_symlinks` is on.
pub fn walk(root: &Path, config: &Config) -> Result<WalkOutcome> {
    let outcome = WalkOutcome::default();
    let canon_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(!config.walk.hidden)
        .parents(config.ignore.respect_gitignore)
        .git_ignore(config.ignore.respect_gitignore)
        .git_global(config.ignore.respect_gitignore)
        .git_exclude(config.ignore.respect_gitignore)
        .ignore(config.ignore.respect_ndexignore)
        .follow_links(config.walk.follow_symlinks);
    if config.ignore.respect_ndexignore {
        builder.add_custom_ignore_filename(NDEXIGNORE_FILE);
    }
    // Never descend into our own index directory (or the reindex staging copy), and
    // never follow a symlink whose target resolves outside the root (containment).
    builder.filter_entry(move |entry| {
        let name = entry.file_name();
        if name == std::ffi::OsStr::new(NDEX_DIR) || name == std::ffi::OsStr::new(NDEX_OLD_DIR) {
            return false;
        }
        if !entry.path_is_symlink() {
            return true;
        }
        match std::fs::canonicalize(entry.path()) {
            Ok(target) if target.starts_with(&canon_root) => true,
            Ok(target) => {
                tracing::debug!(
                    path = %entry.path().display(),
                    target = %target.display(),
                    "skipping symlink escaping the index root"
                );
                false
            }
            Err(err) => {
                tracing::debug!(
                    path = %entry.path().display(),
                    %err,
                    "skipping unresolvable symlink"
                );
                false
            }
        }
    });

    for result in builder.build() {
        let entry = match result {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(%err, "skipping unreadable path during walk");
                continue;
            }
        };
        let Some(file_type) = entry.file_type() else {
            continue; // stdin / special — not a real path
        };
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(err) => {
                tracing::warn!(path = %entry.path().display(), %err, "stat failed; skipping");
                continue;
            }
        };
        let path = NdexPath::from_os_str(entry.path().as_os_str());
        if file_type.is_file() {
            outcome.files.insert(path, file_entry(&meta));
        } else if file_type.is_dir() {
            outcome.dirs.insert(path, dir_entry(&meta));
        }
        // Symlinks (when not followed) and other special files are intentionally skipped.
    }

    tracing::debug!(
        files = outcome.files.len(),
        dirs = outcome.dirs.len(),
        "walk complete"
    );
    Ok(outcome)
}

/// Estimated bytes of reconciliation state per file (PRD §11.1).
const BYTES_PER_FILE: u64 = 500;

/// Abort if estimated reconciliation memory (~500 B/file) would exceed 75% of total RAM
/// (PRD §11.1). Uses `rustix` `sysinfo`; a query failure is non-fatal.
pub fn preflight_memory(estimated_files: u64) -> Result<()> {
    let info = rustix::system::sysinfo();
    let unit = u128::from(info.mem_unit.max(1));
    let total = u128::from(info.totalram) * unit;
    if total == 0 {
        return Ok(()); // sysinfo unavailable — do not block
    }
    let needed = u128::from(estimated_files) * u128::from(BYTES_PER_FILE);
    if needed > total / 4 * 3 {
        return Err(NdexError::Other(format!(
            "estimated reconciliation memory ({needed} B) exceeds 75% of system RAM ({total} B)"
        )));
    }
    Ok(())
}

/// Warn if the estimated index size (~0.5% of data) exceeds free space on the `.ndex/`
/// filesystem (PRD §11.1). Advisory only — returns `Ok` after logging.
pub fn preflight_disk(root: &Path, total_bytes: u64) -> Result<()> {
    let stat = rustix::fs::statvfs(root).map_err(std::io::Error::from)?;
    let free = u128::from(stat.f_bavail) * u128::from(stat.f_frsize);
    let estimated_index = u128::from(total_bytes) / 200;
    if estimated_index > free {
        tracing::warn!(
            estimated_index_bytes = estimated_index as u64,
            free_bytes = free as u64,
            "estimated index size may exceed free space on the index filesystem"
        );
    }
    Ok(())
}
