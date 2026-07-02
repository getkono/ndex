//! Crash recovery at the start of every reconcile run (PRD §11.2).

use ndex_core::error::Result;
use ndex_store::Store;

/// Restore the crash-safety invariant (`status = Indexed` ⟺ chunks durably committed;
/// `index_progress` row ⟹ committed for that index) after a possible interruption.
///
/// A crash mid-run can leave files whose `index_progress` rows exist although their
/// status is neither `Indexed` nor `Skipped` — e.g. a previously indexed file that was
/// being reprocessed (status reset to `Pending`) or deleted (status `Deleted` staged
/// before the FTS commit). For each such file the possibly-stale FTS documents are
/// purged (`delete_file`), the purge is committed, and the orphaned progress rows are
/// removed. The files themselves stay in their non-`Indexed` status, so a later walk
/// reprocesses them; delete-before-add in Phase 3 keeps that idempotent.
///
/// Ordering matters: the FTS purge is committed **before** the progress rows are
/// cleared, so a crash inside `recover` itself leaves the candidates detectable and the
/// next run simply redoes the (idempotent) purge. Called by `Reconciler::run` at the
/// start of every non-dry run; a no-op (no writes) on a clean index.
pub fn recover(store: &mut Store) -> Result<()> {
    let orphans = store.manifest.recovery_candidates()?;
    if orphans.is_empty() {
        return Ok(());
    }
    for &file_id in &orphans {
        store.fts.delete_file(file_id)?;
    }
    store.fts.commit()?;
    for &file_id in &orphans {
        store.manifest.clear_progress(file_id)?;
    }
    tracing::info!(
        files = orphans.len(),
        "crash recovery purged uncommitted index state"
    );
    Ok(())
}
