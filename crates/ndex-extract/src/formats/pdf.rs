//! PDF extraction via `pdf_oxide`, with an optional `pdfium` fallback (PRD §4.4).
//!
//! Image-only PDFs (< 20 chars extracted) → `status=4` `[DEFERRED]` (OCR is v0.3); encrypted
//! PDFs → `status=4` `[UNSUPPORTED]` (PRD §4.8). Populates `doc_meta` from the Info dictionary.

use ndex_core::error::Result;

use crate::extractor::{ExtractCtx, Extraction, Extractor};

/// Extracts text + document metadata from PDF files.
pub struct PdfExtractor;

impl Extractor for PdfExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        // TODO(skeleton): pdf_oxide text + Info dict; fall back to pdfium (feature `pdfium`).
        let _ = (bytes, ctx);
        todo!()
    }
}
