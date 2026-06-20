//! Token counting abstraction.

/// Counts model tokens in a string.
///
/// Defined in `ndex-core` (not `ndex-embed`) so that the extractor's chunker can
/// size chunks in model tokens *without* depending on the embedding crate — this is
/// what keeps the `ndex-extract` → `ndex-embed` edge out of the dependency graph.
/// Implemented by `ndex-embed`'s tokenizer.
pub trait TokenCounter {
    /// Number of model tokens the text encodes to.
    fn count(&self, text: &str) -> usize;
}
