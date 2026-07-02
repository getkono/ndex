//! `ndex-embed` — embedding inference and model management.
//!
//! Wraps ONNX Runtime (`ort`) for batched CPU inference ([`Embedder`]), the HuggingFace
//! tokenizer ([`Tokenizer`], which implements [`ndex_core::TokenCounter`]), and the offline
//! model registry/fetcher ([`model`]). Depends only on `ndex-core`.

pub mod embedder;
pub mod model;
pub mod tokenizer;

pub use embedder::{Embed, Embedder};
pub use model::{ModelInfo, REGISTRY, list, lookup, model_path, models_dir};
pub use tokenizer::{MAX_QUERY_TOKENS, Tokenizer};
