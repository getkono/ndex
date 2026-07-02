//! Core-native progress reporting.
//!
//! The reconciler emits [`ProgressUpdate`]s through a [`ProgressSink`]. `ndex-remote`
//! provides a sink that maps these to the wire `ProgressEvent`, so `ndex-reconcile`
//! never needs to depend on `ndex-protocol`.

use serde::{Deserialize, Serialize};

/// The reconciliation phase an update belongs to (PRD §13.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgressKind {
    Walk,
    Diff,
    Extract,
    Embed,
    Fts,
    Meta,
}

/// A progress update for a phase, with optional per-worker children.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressUpdate {
    pub kind: ProgressKind,
    pub current: u64,
    pub total: Option<u64>,
    pub message: Option<String>,
    pub children: Vec<ProgressChildUpdate>,
}

/// A sub-task within a phase (e.g. an extraction worker or the embed sub-pipeline).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressChildUpdate {
    pub label: String,
    pub current: u64,
    pub total: Option<u64>,
    pub message: Option<String>,
}

/// Receives progress updates. The reconciler calls [`ProgressSink::emit`]; the server
/// forwards updates to the client (or a [`NullSink`] discards them for local runs).
pub trait ProgressSink: Send + Sync {
    fn emit(&self, update: &ProgressUpdate);
}

/// A sink that discards all updates.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

impl ProgressSink for NullSink {
    fn emit(&self, _update: &ProgressUpdate) {}
}
