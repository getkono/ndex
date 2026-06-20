//! Source-code extraction via `tree-sitter` (PRD §4.4, §4.5).
//!
//! Top-level declarations (functions, classes, impls, modules) are section-level chunk
//! boundaries; the enclosing function/class name is propagated as heading context. Languages
//! without a bundled grammar fall through to the plaintext extractor (PRD §4.5).

use ndex_core::error::Result;

use crate::extractor::{ExtractCtx, Extraction, Extractor};

/// Extracts AST-structured blocks from source code.
pub struct CodeExtractor;

impl Extractor for CodeExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        let _ = (bytes, ctx);
        todo!()
    }
}

/// Resolve a language name (from [`crate::mime::extension_language`] / shebang) to its bundled
/// tree-sitter grammar (PRD §4.4 v0.1 grammar set). `None` ⇒ fall through to plaintext.
pub fn language_for(lang: &str) -> Option<tree_sitter::Language> {
    // TODO(skeleton): match lang → tree_sitter_rust::LANGUAGE.into(), tree_sitter_python::…, etc.
    let _ = lang;
    todo!()
}
