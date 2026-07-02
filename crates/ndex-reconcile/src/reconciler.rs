//! The reconciliation orchestrator (PRD §11).

use ndex_core::error::Result;
use ndex_core::progress::{ProgressKind, ProgressSink, ProgressUpdate};
use ndex_embed::Embed;
use ndex_store::{RunKind, Store};

/// Number of reconciliation-run rows to retain (PRD §10.1).
const RUN_HISTORY: u32 = 1000;

/// Current wall-clock time in unix nanoseconds.
fn now_ns() -> i64 {
    jiff::Timestamp::now().as_nanosecond() as i64
}

/// Emit a single-phase progress marker.
fn phase(sink: &dyn ProgressSink, kind: ProgressKind, message: &str) {
    sink.emit(&ProgressUpdate {
        kind,
        current: 0,
        total: None,
        message: Some(message.to_string()),
        children: Vec::new(),
    });
}

/// Options for a reconciliation run. The server maps the wire `IndexOptions` to this (keeping
/// `ndex-reconcile` independent of `ndex-protocol`).
#[derive(Debug, Clone, Default)]
pub struct ReconcileOptions {
    pub full: bool,
    pub verify: bool,
    pub dry_run: bool,
    pub jobs: Option<usize>,
    pub batch_size: Option<usize>,
    pub no_vectors: bool,
    pub max_file_size: Option<u64>,
    pub only_new: bool,
}

/// Statistics from a reconciliation run. The server maps this to the wire `IndexStats`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileStats {
    pub new: u64,
    pub modified: u64,
    pub deleted: u64,
    pub unchanged: u64,
    pub processed: u64,
    pub failed: u64,
    pub skipped: u64,
    pub duration_ms: u64,
    pub timed_out: bool,
}

/// Drives the three-phase reconciliation (walk → diff → process), records the run in
/// `reconciliation_runs`, and prunes history (PRD §11).
pub struct Reconciler<'a> {
    store: &'a mut Store,
    embedder: Option<&'a dyn Embed>,
}

impl<'a> Reconciler<'a> {
    /// Bind a reconciler to an open store and an optional embedder (`None` ⇒ `--no-vectors`).
    pub fn new(store: &'a mut Store, embedder: Option<&'a dyn Embed>) -> Self {
        Self { store, embedder }
    }

    /// Run a reconciliation, emitting progress through `sink` (PRD §11).
    pub fn run(
        &mut self,
        options: &ReconcileOptions,
        sink: &dyn ProgressSink,
    ) -> Result<ReconcileStats> {
        let start = now_ns();
        let root = self.store.root().to_path_buf();
        let kind = if options.full {
            RunKind::Full
        } else {
            RunKind::Incremental
        };
        let run_id = self.store.manifest.begin_run(kind, "mtime")?;

        // Phase 1: walk.
        phase(sink, ProgressKind::Walk, "scanning files");
        let walk = crate::walk::walk(&root, &self.store.config)?;
        crate::walk::preflight_memory(walk.files.len() as u64)?;

        // Phase 2: diff.
        phase(sink, ProgressKind::Diff, "computing changes");
        let diff = crate::diff::diff(&self.store.manifest, &walk)?;

        // Phase 3: process (skipped on a dry run).
        let mut stats = if options.dry_run {
            ReconcileStats {
                new: diff.new.len() as u64,
                modified: diff.modified.len() as u64,
                deleted: diff.deleted.len() as u64,
                unchanged: diff.unchanged,
                ..ReconcileStats::default()
            }
        } else {
            let stats = crate::process::process(self.store, self.embedder, &diff, sink)?;
            self.store.manifest.finish_run(run_id)?;
            self.store.manifest.touch_last_reconciliation(now_ns())?;
            self.store.manifest.prune_reconciliation_runs(RUN_HISTORY)?;
            stats
        };

        stats.duration_ms = (now_ns() - start).max(0) as u64 / 1_000_000;
        tracing::info!(
            new = stats.new,
            modified = stats.modified,
            deleted = stats.deleted,
            processed = stats.processed,
            failed = stats.failed,
            duration_ms = stats.duration_ms,
            "reconciliation complete"
        );
        Ok(stats)
    }
}
