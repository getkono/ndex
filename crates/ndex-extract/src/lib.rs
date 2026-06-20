//! `ndex-extract` — MIME routing, format extraction, chunking, and archive safety.
//!
//! Detects a file's type ([`mime`]), transcodes/normalizes text ([`encoding`]), routes to a
//! per-format [`Extractor`] ([`formats`]), splits the normalized blocks into chunks
//! ([`Chunker`]), and enforces archive-bomb/traversal safety ([`archive_safety`]). The chunker
//! counts tokens through `ndex_core::TokenCounter`, so this crate does not depend on
//! `ndex-embed`. Depends only on `ndex-core`.

pub mod archive_safety;
pub mod chunk;
pub mod encoding;
pub mod extractor;
pub mod formats;
pub mod lang;
pub mod mime;

pub use chunk::Chunker;
pub use extractor::{ExtractCtx, Extraction, Extractor, router};
