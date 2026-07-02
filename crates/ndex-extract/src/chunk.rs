//! Recursive, structure-aware chunking (PRD §4.5).

use ndex_core::config::Chunking;
use ndex_core::model::{Block, Chunk};
use ndex_core::tokens::TokenCounter;

/// Splits ordered [`Block`]s into target-sized [`Chunk`]s.
///
/// Boundary priority (largest semantic unit first): section/heading → paragraph → sentence →
/// word. Small blocks are merged up to `target_tokens`; large blocks are split with
/// `overlap_tokens` overlap; the most recent heading is propagated as context (PRD §4.5).
pub struct Chunker<'a> {
    tokens: &'a dyn TokenCounter,
    config: &'a Chunking,
}

impl<'a> Chunker<'a> {
    /// Create a chunker bound to a token counter and chunking config.
    pub fn new(tokens: &'a dyn TokenCounter, config: &'a Chunking) -> Self {
        Self { tokens, config }
    }

    /// Chunk a file's blocks into ordered chunks (PRD §4.5).
    ///
    /// v0.1 packs each block's words into ~`target_tokens` windows with `overlap_tokens` of
    /// overlap, sizing by the injected [`TokenCounter`]. Recursive cross-block merging and
    /// heading propagation are follow-up refinements.
    pub fn chunk(&self, file_id: i64, blocks: &[Block]) -> Vec<Chunk> {
        let target = self.config.target_tokens.max(1);
        let overlap = self.config.overlap_tokens.min(target.saturating_sub(1));
        let mut chunks = Vec::new();
        let mut ord: u32 = 0;

        for block in blocks {
            let spans = word_spans(&block.text);
            if spans.is_empty() {
                continue;
            }
            let toks: Vec<usize> = spans
                .iter()
                .map(|&(s, e)| self.tokens.count(&block.text[s..e]).max(1))
                .collect();

            let mut i = 0;
            while i < spans.len() {
                let mut j = i;
                let mut count = 0;
                while j < spans.len() && (j == i || count + toks[j] <= target) {
                    count += toks[j];
                    j += 1;
                }
                let text = block.text[spans[i].0..spans[j - 1].1].to_string();
                chunks.push(Chunk {
                    file_id,
                    chunk_ord: ord,
                    byte_start: block.byte_start + spans[i].0 as u64,
                    byte_end: block.byte_start + spans[j - 1].1 as u64,
                    block_type: block.block_type.clone(),
                    text,
                });
                ord += 1;
                if j >= spans.len() {
                    break;
                }
                // Step the window start back by `overlap` tokens, always making progress.
                let mut k = j;
                let mut back = 0;
                while k > i + 1 && back < overlap {
                    k -= 1;
                    back += toks[k];
                }
                i = k.max(i + 1);
            }
        }
        chunks
    }
}

/// Byte ranges of whitespace-delimited words within `text`.
fn word_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                spans.push((s, i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        spans.push((s, text.len()));
    }
    spans
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "skeleton: chunk boundary cases — empty, single token, =target, target+1, all-heading"]
    fn chunk_boundary_conditions() {
        todo!()
    }
}
