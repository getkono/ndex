//! Text-family extractors: plaintext, CSV/TSV, JSON, SQL, logs (PRD §4.5).
//!
//! These share the plaintext path but differ in chunk boundaries: plaintext recurses
//! `\n\n` > `\n` > `. ` > ` `; CSV is record-based (header propagation); JSON is variant-aware;
//! SQL splits on `;`; logs batch by timestamped lines.

use ndex_core::error::Result;
use ndex_core::model::{Block, BlockType};

use crate::extractor::{ExtractCtx, Extraction, Extractor};
use crate::{encoding, lang};

/// Shared text-family extraction: decode → NFC-normalize → paragraph blocks → language detect.
///
/// Plaintext, CSV, JSON, SQL, and log files all share this path in v0.1; their format-specific
/// boundary strategies (record/statement/line batching) are follow-up refinements (PRD §4.5).
pub(crate) fn text_extraction(bytes: &[u8], _ctx: &ExtractCtx<'_>) -> Result<Extraction> {
    let decoded = encoding::decode_to_utf8(bytes);
    let normalized = encoding::nfc_normalize(&decoded);
    let lang = lang::detect(&normalized);
    Ok(Extraction {
        blocks: paragraph_blocks(&normalized),
        doc_meta: None,
        media_meta: None,
        lang,
    })
}

/// Split normalized text into paragraph blocks on blank lines, tracking byte offsets.
fn paragraph_blocks(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut offset = 0usize;
    for part in text.split("\n\n") {
        let lead = part.len() - part.trim_start().len();
        let content = part.trim();
        if !content.is_empty() {
            let start = offset + lead;
            blocks.push(Block {
                block_type: BlockType::Paragraph,
                text: content.to_string(),
                byte_start: start as u64,
                byte_end: (start + content.len()) as u64,
                heading_path: Vec::new(),
            });
        }
        offset += part.len() + 2; // account for the consumed "\n\n" delimiter
    }
    blocks
}

/// Plaintext and config/markup formats (YAML, TOML, INI, rST, AsciiDoc, LaTeX) — PRD §4.8.
pub struct PlaintextExtractor;

impl Extractor for PlaintextExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        text_extraction(bytes, ctx)
    }
}

/// CSV/TSV record-based extractor (delimiter auto-detected; header propagated) — PRD §4.5.
pub struct CsvExtractor;

impl Extractor for CsvExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        text_extraction(bytes, ctx)
    }
}

/// JSON extractor, variant-aware: single object / array / NDJSON (PRD §4.8).
pub struct JsonExtractor;

impl Extractor for JsonExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        text_extraction(bytes, ctx)
    }
}

/// SQL statement-based extractor (splits on `;`) — PRD §4.5.
pub struct SqlExtractor;

impl Extractor for SqlExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        text_extraction(bytes, ctx)
    }
}

/// Log extractor: line-batched, timestamp-pattern aware (PRD §4.5).
pub struct LogExtractor;

impl Extractor for LogExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        text_extraction(bytes, ctx)
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
