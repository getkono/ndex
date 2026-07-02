//! Semantic vector index over usearch + a sidecar mapping (PRD §10.3).

use std::path::Path;

use half::f16;
use ndex_core::error::Result;

/// Magic header for the sidecar file (PRD §10.3).
///
/// Distinct from the IPC `MAGIC_PREAMBLE` in `ndex-core::constants` — do not conflate.
pub const SIDECAR_MAGIC: &[u8; 8] = b"NDEXVEC\0";

/// One sidecar entry: a usearch label mapped to `(file_id, chunk_ord)`.
///
/// Serialized as a fixed 24-byte record on disk (PRD §10.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidecarEntry {
    pub label: u64,
    pub file_id: i64,
    pub chunk_ord: u32,
}

/// The sidecar: a 128-byte header (magic, version, count, model, dims) followed by
/// fixed-size [`SidecarEntry`] records. Saved *before* the usearch index (PRD §10.3).
#[derive(Debug, Default)]
pub struct Sidecar {
    entries: Vec<SidecarEntry>,
}

impl Sidecar {
    /// An empty sidecar.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the sidecar has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Load a sidecar from disk, validating the magic header (PRD §10.3).
    pub fn load(path: &Path) -> Result<Self> {
        let _ = path;
        todo!()
    }

    /// Save the sidecar via temp-file + atomic rename (PRD §10.3).
    pub fn save(&self, path: &Path) -> Result<()> {
        let _ = path;
        todo!()
    }
}

/// A semantic search hit (inner-product distance over L2-normalized vectors).
#[derive(Debug, Clone, PartialEq)]
pub struct VecHit {
    pub file_id: i64,
    pub chunk_ord: u32,
    pub distance: f32,
}

/// usearch HNSW index plus its [`Sidecar`] (PRD §10.3).
///
/// Writes go through a single instance; `view()`-based reads are lock-free (PRD §10.3).
pub struct VectorIndex {
    idx: usearch::Index,
    sidecar: Sidecar,
}

impl VectorIndex {
    /// Open (or create) the vector index under `dir` for `dims`-dimensional vectors.
    pub fn open_or_create(dir: &Path, dims: usize) -> Result<Self> {
        let _ = (dir, dims);
        todo!()
    }

    /// Add a vector for `(file_id, chunk_ord)` and append a sidecar entry.
    pub fn add(&mut self, file_id: i64, chunk_ord: u32, vector: &[f16]) -> Result<()> {
        let _ = (file_id, chunk_ord, vector);
        todo!()
    }

    /// k-nearest-neighbor query, resolving labels through the sidecar.
    pub fn search(&self, vector: &[f16], k: usize) -> Result<Vec<VecHit>> {
        let _ = (vector, k);
        todo!()
    }

    /// Tombstone all vectors belonging to a file (PRD §13.8 — space reclaimed by compact).
    pub fn tombstone(&mut self, file_id: i64) -> Result<()> {
        let _ = file_id;
        todo!()
    }

    /// Number of live vectors (usearch `size()`).
    pub fn len(&self) -> usize {
        todo!()
    }

    /// Whether the index is empty — drives the auto-mode FTS fallback (PRD §16.3).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Persist the index: **sidecar first**, then the usearch index, each via temp+rename
    /// (PRD §10.3 save ordering).
    pub fn save(&self, dir: &Path) -> Result<()> {
        let _ = dir;
        todo!()
    }

    /// Load and validate that the sidecar count matches usearch `size()`, auto-repairing the
    /// sidecar-ahead case when the gap is ≤ 100 (PRD §10.3).
    pub fn load_and_validate(dir: &Path) -> Result<Self> {
        let _ = dir;
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_sidecar_reports_empty() {
        let s = Sidecar::new();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    #[ignore = "skeleton: implement save (sidecar-first) → load_and_validate count check"]
    fn save_then_load_validates_counts() {
        todo!()
    }
}
