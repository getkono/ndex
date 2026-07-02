//! Phase 2 — metadata diff against the manifest (PRD §11.1).

use ndex_core::error::Result;
use ndex_core::path::NdexPath;
use ndex_store::{Change, Manifest};

use crate::walk::WalkOutcome;

/// Phase 2 output: the set of changes to apply (PRD §11.1).
#[derive(Debug, Default)]
pub struct DiffOutcome {
    /// Paths present on disk but not in the manifest.
    pub new: Vec<NdexPath>,
    /// Paths needing (re)processing: `(size, mtime_ns)` changed, or metadata-unchanged
    /// rows still `Pending` / `FailedTransient` under the retry limit (PRD §11.5).
    pub modified: Vec<NdexPath>,
    /// `file_id`s present in the manifest but no longer on disk (→ `status=3`).
    pub deleted: Vec<i64>,
    /// Count of unchanged files (count only; their manifest rows are not touched).
    pub unchanged: u64,
}

/// Classify walked entries against the manifest with a sequential loop (one point query
/// per file plus one `live_files` scan for deletions). Retry eligibility for
/// metadata-unchanged rows is decided by [`Manifest::classify`] using `max_retries`
/// (config `extraction.max_retries`). Read-only: a dry run may safely diff.
pub fn diff(manifest: &Manifest, walk: &WalkOutcome, max_retries: u32) -> Result<DiffOutcome> {
    let mut out = DiffOutcome::default();

    for entry in &walk.files {
        match manifest.classify(entry.key(), entry.value(), max_retries)? {
            Change::New => out.new.push(entry.key().clone()),
            Change::Modified | Change::Retry => out.modified.push(entry.key().clone()),
            Change::Unchanged => out.unchanged += 1,
            Change::Deleted => {}
        }
    }

    // Files present in the manifest but no longer on disk are deletions.
    for (file_id, path) in manifest.live_files()? {
        if !walk.files.contains_key(&path) {
            out.deleted.push(file_id);
        }
    }

    tracing::debug!(
        new = out.new.len(),
        modified = out.modified.len(),
        deleted = out.deleted.len(),
        unchanged = out.unchanged,
        "diff complete"
    );
    Ok(out)
}
