//! Markdown extraction via `pulldown-cmark` (PRD §4.5).
//!
//! Structure signals: headings, code blocks, lists. `title` ← first `# heading`. YAML/TOML
//! frontmatter is indexed as text in v0.1 (structured extraction deferred to v0.2, PRD §4.5).

use ndex_core::error::Result;

use crate::extractor::{ExtractCtx, Extraction, Extractor};

/// Extracts structured blocks from Markdown.
pub struct MarkdownExtractor;

impl Extractor for MarkdownExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        // v0.1 indexes Markdown as text (fully FTS-searchable); structured heading/code-block
        // extraction via pulldown-cmark is a follow-up refinement (PRD §4.5).
        crate::formats::text::text_extraction(bytes, ctx)
    }
}
