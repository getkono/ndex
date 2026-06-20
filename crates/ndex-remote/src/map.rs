//! Mappings between wire types (`ndex-protocol`) and engine types (`ndex-reconcile`).
//!
//! These keep `ndex-reconcile` independent of the wire protocol: the server translates at the
//! boundary (PRD design).

use ndex_protocol::{IndexOptions, IndexStats};
use ndex_reconcile::{ReconcileOptions, ReconcileStats};

/// Map wire [`IndexOptions`] to engine [`ReconcileOptions`].
pub fn to_reconcile_options(o: &IndexOptions) -> ReconcileOptions {
    ReconcileOptions {
        full: o.full,
        verify: o.verify,
        dry_run: o.dry_run,
        jobs: o.jobs.map(|j| j as usize),
        batch_size: o.batch_size.map(|b| b as usize),
        no_vectors: o.no_vectors,
        max_file_size: o.max_file_size,
        only_new: o.only_new,
    }
}

/// Map engine [`ReconcileStats`] to wire [`IndexStats`].
pub fn to_index_stats(s: &ReconcileStats) -> IndexStats {
    IndexStats {
        new: s.new,
        modified: s.modified,
        deleted: s.deleted,
        unchanged: s.unchanged,
        processed: s.processed,
        failed: s.failed,
        skipped: s.skipped,
        duration_ms: s.duration_ms,
        timed_out: s.timed_out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_options_map_through() {
        let wire = IndexOptions {
            full: true,
            jobs: Some(8),
            batch_size: Some(64),
            no_vectors: true,
            max_file_size: Some(1 << 30),
            only_new: true,
            ..Default::default()
        };
        let eng = to_reconcile_options(&wire);
        assert!(eng.full && eng.no_vectors && eng.only_new);
        assert_eq!(eng.jobs, Some(8));
        assert_eq!(eng.max_file_size, Some(1 << 30));
    }

    #[test]
    fn stats_map_through() {
        let eng = ReconcileStats {
            new: 10,
            processed: 9,
            failed: 1,
            duration_ms: 42,
            ..Default::default()
        };
        let wire = to_index_stats(&eng);
        assert_eq!(wire.new, 10);
        assert_eq!(wire.processed, 9);
        assert_eq!(wire.failed, 1);
        assert_eq!(wire.duration_ms, 42);
    }
}
