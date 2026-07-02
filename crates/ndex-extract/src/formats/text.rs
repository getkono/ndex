//! Text-family extractors: plaintext, CSV/TSV, JSON, SQL, logs (PRD §4.5).
//!
//! These share the plaintext path but differ in chunk boundaries: plaintext recurses
//! `\n\n` > `\n` > `. ` > ` `; CSV is record-based (header propagation); JSON is variant-aware;
//! SQL splits on `;`; logs batch by timestamped lines.

use ndex_core::error::Result;

use crate::extractor::{ExtractCtx, Extraction, Extractor};

/// Plaintext and config/markup formats (YAML, TOML, INI, rST, AsciiDoc, LaTeX) — PRD §4.8.
pub struct PlaintextExtractor;

impl Extractor for PlaintextExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        let _ = (bytes, ctx);
        todo!()
    }
}

/// CSV/TSV record-based extractor (delimiter auto-detected; header propagated) — PRD §4.5.
pub struct CsvExtractor;

impl Extractor for CsvExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        let _ = (bytes, ctx);
        todo!()
    }
}

/// JSON extractor, variant-aware: single object / array / NDJSON (PRD §4.8).
pub struct JsonExtractor;

impl Extractor for JsonExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        let _ = (bytes, ctx);
        todo!()
    }
}

/// SQL statement-based extractor (splits on `;`) — PRD §4.5.
pub struct SqlExtractor;

impl Extractor for SqlExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        let _ = (bytes, ctx);
        todo!()
    }
}

/// Log extractor: line-batched, timestamp-pattern aware (PRD §4.5).
pub struct LogExtractor;

impl Extractor for LogExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        let _ = (bytes, ctx);
        todo!()
    }
}

/// Top-level shape of a JSON document (PRD §4.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonVariant {
    /// Starts with `{` — a single object.
    Object,
    /// Starts with `[` — an array of values.
    Array,
}

/// Classify a JSON document by its first non-whitespace byte (PRD §4.8).
///
/// NDJSON disambiguation (multiple top-level values, one per line) is refined in the extractor.
pub fn json_variant(bytes: &[u8]) -> Option<JsonVariant> {
    let s = std::str::from_utf8(bytes).ok()?;
    match s.trim_start().bytes().next()? {
        b'{' => Some(JsonVariant::Object),
        b'[' => Some(JsonVariant::Array),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_variant_by_first_char() {
        assert_eq!(json_variant(b"  {\"a\":1}"), Some(JsonVariant::Object));
        assert_eq!(json_variant(b"[1,2,3]"), Some(JsonVariant::Array));
        assert_eq!(json_variant(b"not json"), None);
    }
}
