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
    pub fn chunk(&self, file_id: i64, blocks: &[Block]) -> Vec<Chunk> {
        let _ = (file_id, blocks, self.tokens, self.config);
        todo!()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "skeleton: chunk boundary cases — empty, single token, =target, target+1, all-heading"]
    fn chunk_boundary_conditions() {
        todo!()
    }
}
