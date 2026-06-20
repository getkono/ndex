//! Phase 1 — parallel filesystem walk (PRD §11.1).

use std::path::Path;

use dashmap::DashMap;
use ndex_core::config::Config;
use ndex_core::error::Result;
use ndex_core::model::{DirWalkEntry, WalkEntry};
use ndex_core::path::NdexPath;

/// Phase 1 output: filesystem metadata for files and directories under the root (PRD §11.1).
#[derive(Debug, Default)]
pub struct WalkOutcome {
    pub files: DashMap<NdexPath, WalkEntry>,
    pub dirs: DashMap<NdexPath, DirWalkEntry>,
}

/// Walk `root` in parallel via the `ignore` crate, honoring `.gitignore`/`.ndexignore`, skipping
/// non-regular files, and detecting symlink cycles by `(dev, inode)` (PRD §11.1).
pub fn walk(root: &Path, config: &Config) -> Result<WalkOutcome> {
    // TODO(skeleton): ignore::WalkBuilder parallel walk → DashMaps.
    let _ = (root, config);
    todo!()
}

/// Abort if estimated reconciliation memory (~500 B/file) would exceed 75% of available RAM
/// (PRD §11.1). Uses `rustix` `sysinfo`.
pub fn preflight_memory(estimated_files: u64) -> Result<()> {
    let _ = estimated_files;
    todo!()
}

/// Warn if the estimated index size (~0.5% of data) exceeds free space on the `.ndex/`
/// filesystem (PRD §11.1). Uses `statvfs`.
pub fn preflight_disk(root: &Path, total_bytes: u64) -> Result<()> {
    let _ = (root, total_bytes);
    todo!()
}
