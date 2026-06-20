//! Image metadata extraction via `kamadak-exif` + `image` (PRD §4.8 EXIF matrix).
//!
//! Produces a `media_meta` row (no `doc_meta`, no chunks): EXIF for JPEG/TIFF/HEIC/raw, and
//! `width`/`height` for all decodable formats. Video/audio are handled by the reconciler as
//! `status=1` with empty `media_meta` in v0.1 (PRD §4.6).

use ndex_core::error::Result;

use crate::extractor::{ExtractCtx, Extraction, Extractor};

/// Extracts EXIF + pixel dimensions into `media_meta`.
pub struct ImageExtractor;

impl Extractor for ImageExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        // TODO(skeleton): exif::Reader for EXIF; image crate for width/height.
        let _ = (bytes, ctx);
        todo!()
    }
}
