//! Archive extraction: ZIP, TAR family, single-file GZ/BZ2/XZ (PRD §4.6, §4.9).
//!
//! Members are streamed one at a time through the standard pipeline with the safety limits in
//! [`crate::archive_safety`]. 7Z/RAR are metadata-only in v0.1. OOXML (DOCX/XLSX/PPTX) is not
//! recursively unpacked — dedicated extractors own those (PRD §4.6).

use ndex_core::error::Result;

use crate::extractor::{ExtractCtx, Extraction, Extractor};

/// Detected archive container format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    Gz,
    Bz2,
    Xz,
}

/// Extracts and indexes archive members (the reconciler drives recursion + depth, PRD §4.9).
pub struct ArchiveExtractor;

impl Extractor for ArchiveExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        // NOTE: the archive extractor's primary output is member files routed back through the
        // pipeline by the reconciler; this returns archive-level blocks/metadata only.
        let _ = (bytes, ctx);
        todo!()
    }
}
