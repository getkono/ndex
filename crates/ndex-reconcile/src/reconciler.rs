//! The reconciliation orchestrator (PRD §11).

use ndex_core::error::Result;
use ndex_core::progress::ProgressSink;
use ndex_embed::Embed;
use ndex_store::Store;

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
        let _ = (&self.store, self.embedder, options, sink);
        todo!()
    }
}
