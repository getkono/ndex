//! Phase 3 — extract, hash, chunk, embed, index (PRD §11.1, §11.2).

use std::io;

use ndex_core::error::Result;
use ndex_core::model::WalkEntry;
use ndex_core::progress::ProgressSink;
use ndex_core::status::FileStatus;
use ndex_embed::Embed;
use ndex_store::Store;

use crate::diff::DiffOutcome;
use crate::reconciler::ReconcileStats;

/// Classify an extraction-time I/O error (PRD §11.1):
/// `ENOENT` (the file was removed between walk and extraction) → [`FileStatus::Deleted`];
/// every other I/O error → [`FileStatus::FailedTransient`] for retry.
pub fn classify_io_error(err: &io::Error) -> FileStatus {
    if err.kind() == io::ErrorKind::NotFound {
        FileStatus::Deleted
    } else {
        FileStatus::FailedTransient
    }
}

/// TOCTOU guard (PRD §11.1): an extraction result is valid only if `(size, mtime_ns)` are
/// unchanged since the Phase 1 walk; otherwise the file is re-queued as `FailedTransient`.
pub fn restat_unchanged(walked: &WalkEntry, current_size: u64, current_mtime_ns: i64) -> bool {
    walked.size == current_size && walked.mtime_ns == current_mtime_ns
}

/// Phase 3: the bounded `extract → BLAKE3 → chunk → embed → index` pipeline with two-phase commit
/// and crash-safe write ordering (PRD §11.1, §11.2). Extraction routes through `ndex-extract`;
/// embedding through `ndex-embed`.
pub fn process(
    store: &mut Store,
    embedder: Option<&dyn Embed>,
    diff: &DiffOutcome,
    sink: &dyn ProgressSink,
) -> Result<ReconcileStats> {
    let _ = (store, embedder, diff, sink);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enoent_is_deleted_other_io_is_transient() {
        assert_eq!(
            classify_io_error(&io::Error::from(io::ErrorKind::NotFound)),
            FileStatus::Deleted
        );
        assert_eq!(
            classify_io_error(&io::Error::from(io::ErrorKind::PermissionDenied)),
            FileStatus::FailedTransient
        );
    }

    #[test]
    fn restat_detects_size_or_mtime_change() {
        let walked = WalkEntry {
            size: 100,
            mtime_ns: 5,
            ctime_ns: 0,
            inode: 1,
            dev: 1,
            mode: 0o644,
        };
        assert!(restat_unchanged(&walked, 100, 5));
        assert!(!restat_unchanged(&walked, 101, 5));
        assert!(!restat_unchanged(&walked, 100, 6));
    }
}
