//! Per-format extractors (PRD §4.4, §4.5, §4.8).
//!
//! Each submodule provides one or more [`crate::extractor::Extractor`] implementations.
//! Bodies are `todo!()`; the chosen library and chunking strategy are documented per module.

pub mod archive;
pub mod code;
pub mod docx;
pub mod html;
pub mod image;
pub mod markdown;
pub mod pdf;
pub mod text;
