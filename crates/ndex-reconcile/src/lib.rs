//! `ndex-reconcile` — the three-phase reconciliation engine.
//!
//! Phase 1 `walk` (parallel filesystem traversal), Phase 2 `diff` (metadata diff against the
//! manifest), and Phase 3 `process` (extract → hash → chunk → embed → index). Also provides crash
//! recovery (`recover`) and opportunistic pre-search [`refresh`]. Emits progress through
//! `ndex_core::ProgressSink` and produces engine-native [`ReconcileStats`]; the server maps those
//! to the wire types.

pub mod diff;
pub mod process;
pub mod reconciler;
pub mod recover;
pub mod refresh;
pub mod walk;

pub use diff::{DiffOutcome, diff};
pub use process::{classify_io_error, process, restat_unchanged};
pub use reconciler::{ReconcileOptions, ReconcileStats, Reconciler};
pub use recover::recover;
pub use refresh::{Staleness, quick_reconcile, staleness};
pub use walk::{WalkOutcome, preflight_disk, preflight_memory, walk};
