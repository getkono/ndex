//! HuggingFace tokenizer wrapper + token counting (PRD §4.7).

use std::path::Path;

use ndex_core::constants::QUERY_PREFIX;
use ndex_core::error::Result;
use ndex_core::tokens::TokenCounter;

/// Maximum query length in tokens (model limit, PRD §4.7).
pub const MAX_QUERY_TOKENS: usize = 512;

/// Wraps a `tokenizers::Tokenizer` loaded from a model's `tokenizer.json`.
pub struct Tokenizer {
    inner: tokenizers::Tokenizer,
}

impl Tokenizer {
    /// Load `tokenizer.json` from a model directory.
    pub fn load(tokenizer_json: &Path) -> Result<Self> {
        // TODO(skeleton): tokenizers::Tokenizer::from_file(tokenizer_json).
        let _ = tokenizer_json;
        todo!()
    }

    /// Encode text to token ids.
    pub fn encode(&self, text: &str) -> Result<Vec<u32>> {
        let _ = text;
        todo!()
    }

    /// Prepend the asymmetric query prefix (`"query: "`) used at search time (PRD §4.7).
    pub fn with_query_prefix(query: &str) -> String {
        format!("{QUERY_PREFIX}{query}")
    }

    /// Truncate a token id sequence to at most `max` tokens (PRD §4.7).
    pub fn truncate(ids: Vec<u32>, max: usize) -> Vec<u32> {
        let mut ids = ids;
        ids.truncate(max);
        ids
    }
}

impl TokenCounter for Tokenizer {
    fn count(&self, text: &str) -> usize {
        // TODO(skeleton): self.encode(text).map(|ids| ids.len()).unwrap_or(0)
        let _ = text;
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_prefix_is_applied() {
        assert_eq!(
            Tokenizer::with_query_prefix("quarterly earnings"),
            "query: quarterly earnings"
        );
    }

    #[test]
    fn truncate_caps_length() {
        assert_eq!(Tokenizer::truncate(vec![1, 2, 3, 4, 5], 3), vec![1, 2, 3]);
        assert_eq!(Tokenizer::truncate(vec![1, 2], 5), vec![1, 2]);
    }
}
