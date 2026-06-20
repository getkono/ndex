//! Full-text content index over tantivy (PRD §10.2).

use std::path::Path;

use ndex_core::error::{NdexError, Result};
use ndex_core::model::Chunk;
use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::QueryParser;
use tantivy::schema::{FAST, Field, INDEXED, STORED, STRING, Schema, TEXT, Value};
use tantivy::snippet::SnippetGenerator;
use tantivy::{Index, IndexReader, IndexWriter, TantivyDocument, Term};

/// Heap budget for the single writer thread (PRD §10.2 single-writer model).
const WRITER_HEAP_BYTES: usize = 64 * 1024 * 1024;

/// A full-text search hit.
#[derive(Debug, Clone, PartialEq)]
pub struct FtsHit {
    pub file_id: i64,
    pub chunk_ord: u32,
    pub score: f32,
    pub byte_start: u64,
    pub byte_end: u64,
}

/// The set of schema fields (PRD §10.2), resolved once at open.
#[derive(Debug, Clone, Copy)]
struct Fields {
    file_id: Field,
    chunk_ord: Field,
    body: Field,
    title: Field,
    path_text: Field,
    mime: Field,
    lang: Field,
    byte_start: Field,
    byte_end: Field,
    size: Field,
    mtime: Field,
}

/// Build the schema and resolve the field handles together.
fn build(schema_only: bool) -> (Schema, Option<Fields>) {
    let mut b = Schema::builder();
    let file_id = b.add_i64_field("file_id", INDEXED | STORED | FAST);
    let chunk_ord = b.add_u64_field("chunk_ord", STORED | FAST);
    let body = b.add_text_field("body", TEXT | STORED);
    let title = b.add_text_field("title", TEXT | STORED);
    let path_text = b.add_text_field("path_text", TEXT | STORED);
    let mime = b.add_text_field("mime", STRING | STORED);
    let lang = b.add_text_field("lang", STRING | STORED);
    let byte_start = b.add_u64_field("byte_start", STORED);
    let byte_end = b.add_u64_field("byte_end", STORED);
    let size = b.add_u64_field("size", STORED | FAST);
    let mtime = b.add_i64_field("mtime", STORED | FAST);
    let schema = b.build();
    let fields = if schema_only {
        None
    } else {
        Some(Fields {
            file_id,
            chunk_ord,
            body,
            title,
            path_text,
            mime,
            lang,
            byte_start,
            byte_end,
            size,
            mtime,
        })
    };
    (schema, fields)
}

fn err(e: impl std::fmt::Display) -> NdexError {
    NdexError::Index(e.to_string())
}

fn doc_i64(doc: &TantivyDocument, f: Field) -> i64 {
    doc.get_first(f)
        .and_then(|v| v.as_i64())
        .unwrap_or_default()
}

fn doc_u64(doc: &TantivyDocument, f: Field) -> u64 {
    doc.get_first(f)
        .and_then(|v| v.as_u64())
        .unwrap_or_default()
}

/// The tantivy full-text index.
///
/// Holds the single `IndexWriter` (`Send + !Sync`, one per index — owned by the writer
/// thread) and a cheap-to-clone `IndexReader` for lock-free concurrent search (PRD §10.2).
pub struct FtsIndex {
    index: Index,
    writer: IndexWriter,
    reader: IndexReader,
    fields: Fields,
}

impl FtsIndex {
    /// Build the tantivy schema (fields per PRD §10.2: `file_id`, `chunk_ord`, `body`,
    /// `title`, `path_text`, `mime`, `lang`, `mtime`, `size`, `byte_start`, `byte_end`).
    pub fn build_schema() -> tantivy::schema::Schema {
        build(true).0
    }

    /// Open (or create) the index under `dir`, registering tokenizers (PRD §10.2).
    pub fn open_or_create(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let (schema, fields) = build(false);
        let fields = fields.expect("fields requested");
        let mmap = MmapDirectory::open(dir).map_err(err)?;
        let index = Index::open_or_create(mmap, schema).map_err(err)?;
        let writer = index
            .writer_with_num_threads(1, WRITER_HEAP_BYTES)
            .map_err(err)?;
        let reader = index.reader().map_err(err)?;
        Ok(Self {
            index,
            writer,
            reader,
            fields,
        })
    }

    /// Add one chunk as a tantivy document.
    pub fn add_chunk(
        &mut self,
        file_id: i64,
        chunk: &Chunk,
        mime: &str,
        lang: Option<&str>,
    ) -> Result<()> {
        let mut doc = TantivyDocument::default();
        doc.add_i64(self.fields.file_id, file_id);
        doc.add_u64(self.fields.chunk_ord, u64::from(chunk.chunk_ord));
        doc.add_text(self.fields.body, &chunk.text);
        doc.add_text(self.fields.mime, mime);
        if let Some(l) = lang {
            doc.add_text(self.fields.lang, l);
        }
        doc.add_u64(self.fields.byte_start, chunk.byte_start);
        doc.add_u64(self.fields.byte_end, chunk.byte_end);
        self.writer.add_document(doc).map_err(err)?;
        Ok(())
    }

    /// Delete all documents for a file (PRD §13.8).
    pub fn delete_file(&mut self, file_id: i64) -> Result<()> {
        self.writer
            .delete_term(Term::from_field_i64(self.fields.file_id, file_id));
        Ok(())
    }

    /// Commit pending documents to disk and refresh the reader (PRD §10.2 commit strategy).
    pub fn commit(&mut self) -> Result<()> {
        self.writer.commit().map_err(err)?;
        self.reader.reload().map_err(err)?;
        Ok(())
    }

    /// Schedule a segment merge if the segment count exceeds `segment_threshold` (PRD §16.4).
    ///
    /// v0.1 relies on tantivy's default merge policy; this is a no-op hook kept for the API.
    pub fn maybe_merge(&mut self, segment_threshold: usize) -> Result<()> {
        let _ = segment_threshold;
        Ok(())
    }

    /// Run a BM25 query over `body`+`title`, applying the title field boost (PRD §10.7).
    pub fn search(&self, query: &str, limit: usize, title_boost: f32) -> Result<Vec<FtsHit>> {
        let searcher = self.reader.searcher();
        let mut parser =
            QueryParser::for_index(&self.index, vec![self.fields.body, self.fields.title]);
        parser.set_field_boost(self.fields.title, title_boost);
        let parsed = parser.parse_query(query).map_err(err)?;
        let top = searcher
            .search(&parsed, &TopDocs::with_limit(limit).order_by_score())
            .map_err(err)?;
        let mut hits = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let doc: TantivyDocument = searcher.doc(addr).map_err(err)?;
            hits.push(FtsHit {
                file_id: doc_i64(&doc, self.fields.file_id),
                chunk_ord: doc_u64(&doc, self.fields.chunk_ord) as u32,
                score,
                byte_start: doc_u64(&doc, self.fields.byte_start),
                byte_end: doc_u64(&doc, self.fields.byte_end),
            });
        }
        Ok(hits)
    }

    /// Generate a highlighted snippet for a specific `(file_id, chunk_ord)` hit (PRD §10.2).
    pub fn snippet(&self, file_id: i64, chunk_ord: u32, query: &str) -> Result<Option<String>> {
        let searcher = self.reader.searcher();
        let parser = QueryParser::for_index(&self.index, vec![self.fields.body]);
        let parsed = parser.parse_query(query).map_err(err)?;
        let generator =
            SnippetGenerator::create(&searcher, &*parsed, self.fields.body).map_err(err)?;
        let top = searcher
            .search(&parsed, &TopDocs::with_limit(64).order_by_score())
            .map_err(err)?;
        for (_score, addr) in top {
            let doc: TantivyDocument = searcher.doc(addr).map_err(err)?;
            if doc_i64(&doc, self.fields.file_id) == file_id
                && doc_u64(&doc, self.fields.chunk_ord) == u64::from(chunk_ord)
            {
                let snippet = generator.snippet_from_doc(&doc);
                return Ok(Some(snippet.to_html()));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_has_core_fields() {
        let schema = FtsIndex::build_schema();
        assert!(schema.get_field("body").is_ok());
        assert!(schema.get_field("file_id").is_ok());
        assert!(schema.get_field("chunk_ord").is_ok());
    }
}
