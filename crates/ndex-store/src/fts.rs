//! Full-text content index over tantivy (PRD §10.2).

use std::path::Path;

use ndex_core::error::Result;
use ndex_core::model::Chunk;

/// A full-text search hit.
#[derive(Debug, Clone, PartialEq)]
pub struct FtsHit {
    pub file_id: i64,
    pub chunk_ord: u32,
    pub score: f32,
    pub byte_start: u64,
    pub byte_end: u64,
}

/// The tantivy full-text index.
///
/// Holds the single `IndexWriter` (`Send + !Sync`, one per index — owned by the writer
/// thread) and a cheap-to-clone `IndexReader` for lock-free concurrent search (PRD §10.2).
pub struct FtsIndex {
    writer: tantivy::IndexWriter,
    reader: tantivy::IndexReader,
}

impl FtsIndex {
    /// Build the tantivy schema (fields per PRD §10.2: `file_id`, `chunk_ord`, `body`,
    /// `title`, `path_text`, `mime`, `lang`, `mtime`, `size`, `byte_start`, `byte_end`).
    pub fn build_schema() -> tantivy::schema::Schema {
        todo!("declare PRD §10.2 fields and the `default` + `path` tokenizers")
    }

    /// Open (or create) the index under `dir`, registering tokenizers (PRD §10.2).
    pub fn open_or_create(dir: &Path) -> Result<Self> {
        let _ = dir;
        todo!()
    }

    /// Add one chunk as a tantivy document.
    pub fn add_chunk(
        &mut self,
        file_id: i64,
        chunk: &Chunk,
        mime: &str,
        lang: Option<&str>,
    ) -> Result<()> {
        let _ = (file_id, chunk, mime, lang);
        todo!()
    }

    /// Delete all documents for a file (PRD §13.8).
    pub fn delete_file(&mut self, file_id: i64) -> Result<()> {
        let _ = file_id;
        todo!()
    }

    /// Commit pending documents to disk (PRD §10.2 commit strategy).
    pub fn commit(&mut self) -> Result<()> {
        todo!()
    }

    /// Schedule a segment merge if the segment count exceeds `segment_threshold` (PRD §16.4).
    pub fn maybe_merge(&mut self, segment_threshold: usize) -> Result<()> {
        let _ = segment_threshold;
        todo!()
    }

    /// Run a BM25 query, applying the title field boost (PRD §10.7).
    pub fn search(&self, query: &str, limit: usize, title_boost: f32) -> Result<Vec<FtsHit>> {
        let _ = (query, limit, title_boost);
        todo!()
    }

    /// Generate a highlighted snippet for a hit via tantivy's `SnippetGenerator` (PRD §10.2).
    pub fn snippet(&self, file_id: i64, chunk_ord: u32, query: &str) -> Result<Option<String>> {
        let _ = (file_id, chunk_ord, query);
        todo!()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "skeleton: implement FtsIndex open → add_chunk → commit → search round-trip"]
    fn add_and_search_roundtrip() {
        todo!()
    }
}
