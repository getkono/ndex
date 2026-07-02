//! DOCX extraction via `docx-rust` (PRD §4.4).
//!
//! Reads paragraph styles, headings, and tables; falls back to paragraph-boundary splitting on
//! malformed files. Populates `doc_meta` from `docProps/core.xml` + `app.xml` (PRD §10.4).
//! XLSX/PPTX are NOT handled here — they are archive metadata-only in v0.1 (PRD §4.8).

use ndex_core::error::Result;

use crate::extractor::{ExtractCtx, Extraction, Extractor};

/// Extracts text + document metadata from DOCX files.
pub struct DocxExtractor;

impl Extractor for DocxExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        let _ = (bytes, ctx);
        todo!()
    }
}
