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
    use super::*;
    use ndex_core::model::BlockType;
    use proptest::prelude::*;

    /// One token per whitespace-delimited word.
    struct WordCounter;
    impl TokenCounter for WordCounter {
        fn count(&self, text: &str) -> usize {
            text.split_whitespace().count()
        }
    }

    /// One token per 4 bytes, rounded up — a crude stand-in for a subword tokenizer,
    /// so words have varying token counts.
    struct ByteCounter;
    impl TokenCounter for ByteCounter {
        fn count(&self, text: &str) -> usize {
            text.len().div_ceil(4)
        }
    }

    fn cfg(target_tokens: usize, overlap_tokens: usize) -> Chunking {
        Chunking {
            target_tokens,
            overlap_tokens,
            ..Chunking::default()
        }
    }

    fn para(text: &str) -> Block {
        Block {
            block_type: BlockType::Paragraph,
            text: text.to_string(),
            byte_start: 0,
            byte_end: text.len() as u64,
            heading_path: vec![],
        }
    }

    #[test]
    fn chunk_boundary_conditions() {
        let ws = WordCounter;
        let config = cfg(4, 1);
        let chunker = Chunker::new(&ws, &config);

        // Empty input: no blocks, empty blocks, and whitespace-only blocks all yield no chunks.
        assert!(chunker.chunk(1, &[]).is_empty());
        assert!(chunker.chunk(1, &[para("")]).is_empty());
        assert!(chunker.chunk(1, &[para(" \t\n ")]).is_empty());

        // Single token: one chunk spanning exactly that word.
        let chunks = chunker.chunk(1, &[para("hello")]);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_ord, 0);
        assert_eq!(chunks[0].text, "hello");
        assert_eq!((chunks[0].byte_start, chunks[0].byte_end), (0, 5));

        // Exactly target_tokens (4 one-token words): a single chunk, no spill.
        let chunks = chunker.chunk(1, &[para("w0 w1 w2 w3")]);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "w0 w1 w2 w3");

        // target + 1 (5 words): spills into a second window whose start steps back
        // by overlap_tokens (= 1), re-including the last word of the first window.
        let chunks = chunker.chunk(1, &[para("w0 w1 w2 w3 w4")]);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].text, "w0 w1 w2 w3");
        assert_eq!(chunks[1].text, "w3 w4");
        assert_eq!(chunks[1].chunk_ord, 1);

        // All-heading input: heading blocks chunk like any other block — block_type is
        // preserved, heading_path is never consulted, and chunk_ord continues across blocks.
        let headings = [
            Block {
                block_type: BlockType::Heading(1),
                text: "Quarterly Report".to_string(),
                byte_start: 0,
                byte_end: 16,
                heading_path: vec!["Quarterly Report".into()],
            },
            Block {
                block_type: BlockType::Heading(2),
                text: "Overview".to_string(),
                byte_start: 18,
                byte_end: 26,
                heading_path: vec!["Quarterly Report".into(), "Overview".into()],
            },
        ];
        let chunks = chunker.chunk(1, &headings);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].block_type, BlockType::Heading(1));
        assert_eq!(chunks[0].text, "Quarterly Report");
        assert_eq!(chunks[1].block_type, BlockType::Heading(2));
        assert_eq!(chunks[1].chunk_ord, 1);
        assert_eq!((chunks[1].byte_start, chunks[1].byte_end), (18, 26));
    }

    proptest! {
        /// Invariants over a single block, arbitrary text, and arbitrary config —
        /// including the `target_tokens = 0 → 1` and `overlap_tokens ≥ target` clamps.
        #[test]
        fn chunk_invariants_single_block(
            text in any::<String>(),
            target in 0usize..64,
            overlap in 0usize..64,
            by_bytes in any::<bool>(),
        ) {
            let word = WordCounter;
            let bytes = ByteCounter;
            let counter: &dyn TokenCounter = if by_bytes { &bytes } else { &word };
            let config = cfg(target, overlap);
            let chunks = Chunker::new(counter, &config).chunk(3, &[para(&text)]);

            // Replicate the chunker's effective parameters and per-word token counts.
            let target_eff = target.max(1);
            let overlap_eff = overlap.min(target_eff - 1);
            let spans = word_spans(&text);
            let toks: Vec<usize> = spans
                .iter()
                .map(|&(s, e)| counter.count(&text[s..e]).max(1))
                .collect();

            // chunk_ord is strictly monotonic from 0 with no gaps.
            for (i, c) in chunks.iter().enumerate() {
                prop_assert_eq!(c.chunk_ord as usize, i);
            }

            // Every byte range lies within the input and slices to the chunk's own
            // non-empty text (char-boundary safe).
            for c in &chunks {
                let (s, e) = (c.byte_start as usize, c.byte_end as usize);
                prop_assert!(s < e, "empty range {s}..{e}");
                prop_assert!(e <= text.len());
                prop_assert!(text.is_char_boundary(s) && text.is_char_boundary(e));
                prop_assert_eq!(&text[s..e], c.text.as_str());
                prop_assert!(!c.text.is_empty());
            }

            for pair in chunks.windows(2) {
                // Byte ranges are non-decreasing.
                prop_assert!(pair[1].byte_start >= pair[0].byte_start);
                prop_assert!(pair[1].byte_end >= pair[0].byte_end);

                // Realized overlap never exceeds overlap_tokens + one word: the shared
                // tokens minus the first shared word stay strictly under the effective
                // overlap (the back-walk stops at the first word crossing it).
                let shared: Vec<usize> = spans
                    .iter()
                    .zip(&toks)
                    .filter(|&(&(s, e), _)| {
                        s as u64 >= pair[1].byte_start && e as u64 <= pair[0].byte_end
                    })
                    .map(|(_, &t)| t)
                    .collect();
                let shared_total: usize = shared.iter().sum();
                let first_shared = shared.first().copied().unwrap_or(0);
                prop_assert!(
                    shared_total.saturating_sub(first_shared) < overlap_eff.max(1),
                    "realized overlap {shared_total} (first word {first_shared}) exceeds \
                     configured overlap {overlap_eff}"
                );
            }
        }

        /// Invariants across multiple blocks laid out back-to-back in one virtual input.
        #[test]
        fn chunk_invariants_across_blocks(
            texts in prop::collection::vec(any::<String>(), 0..4),
            target in 0usize..32,
            overlap in 0usize..32,
        ) {
            let mut input = String::new();
            let mut blocks = Vec::new();
            for text in &texts {
                let byte_start = input.len() as u64;
                input.push_str(text);
                blocks.push(Block {
                    block_type: BlockType::Paragraph,
                    text: text.clone(),
                    byte_start,
                    byte_end: input.len() as u64,
                    heading_path: vec![],
                });
            }

            let ws = WordCounter;
            let config = cfg(target, overlap);
            let chunks = Chunker::new(&ws, &config).chunk(9, &blocks);

            for (i, c) in chunks.iter().enumerate() {
                prop_assert_eq!(c.chunk_ord as usize, i);
                prop_assert_eq!(c.file_id, 9);
                let (s, e) = (c.byte_start as usize, c.byte_end as usize);
                prop_assert!(s < e && e <= input.len());
                prop_assert!(input.is_char_boundary(s) && input.is_char_boundary(e));
                prop_assert_eq!(&input[s..e], c.text.as_str());
            }
            for pair in chunks.windows(2) {
                prop_assert!(pair[1].byte_start >= pair[0].byte_start);
                prop_assert!(pair[1].byte_end >= pair[0].byte_end);
            }
        }
    }
}
