//! The `Extractor` trait, extraction context/output, and MIME → extractor routing.

use ndex_core::config::Config;
use ndex_core::error::Result;
use ndex_core::model::{Block, DocMeta, MediaMeta};
use ndex_core::path::NdexPath;
use ndex_core::tokens::TokenCounter;

use crate::formats;

/// Context passed to every [`Extractor`].
pub struct ExtractCtx<'a> {
    /// Detected MIME type.
    pub mime: &'a str,
    /// Source path (for language detection, archive member naming, logging).
    pub path: &'a NdexPath,
    /// Token counter used by downstream chunking.
    pub tokens: &'a dyn TokenCounter,
    /// Archive nesting depth (0 for regular files; PRD §4.9).
    pub depth: u8,
    /// Effective server configuration.
    pub config: &'a Config,
}

/// The normalized output of an extractor: ordered blocks plus optional metadata (PRD §4.5).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Extraction {
    pub blocks: Vec<Block>,
    pub doc_meta: Option<DocMeta>,
    pub media_meta: Option<MediaMeta>,
    pub lang: Option<String>,
}

/// A format extractor: raw bytes → normalized blocks + metadata. Object-safe.
pub trait Extractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction>;
}

/// Whether a MIME type is one of the supported archive containers (PRD §4.6).
pub fn is_archive_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/zip"
            | "application/x-tar"
            | "application/gzip"
            | "application/x-gzip"
            | "application/x-bzip2"
            | "application/x-xz"
            | "application/x-7z-compressed"
            | "application/vnd.rar"
    )
}

/// The routing decision for a detected MIME type (PRD §4.4, §4.8).
pub enum Route {
    /// A supported format: extract content with this extractor.
    Extract(Box<dyn Extractor>),
    /// No extractor handles this MIME type (notably `application/octet-stream`); the caller
    /// must not extract and should record the file as skipped (`status=5`, PRD §4.8).
    Skip,
}

/// Route a MIME type to its extractor, or to [`Route::Skip`] when nothing handles it
/// (PRD §4.4, §4.8).
///
/// There is no code-MIME branch: code types such as `text/x-rust` fall through to the
/// `text/*` family branch and are extracted as plaintext.
pub fn router(mime: &str) -> Route {
    match mime {
        "application/pdf" => Route::Extract(Box::new(formats::pdf::PdfExtractor)),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            Route::Extract(Box::new(formats::docx::DocxExtractor))
        }
        "text/markdown" | "text/x-markdown" => {
            Route::Extract(Box::new(formats::markdown::MarkdownExtractor))
        }
        "text/html"
        | "application/xhtml+xml"
        | "application/xml"
        | "text/xml"
        | "image/svg+xml" => Route::Extract(Box::new(formats::html::HtmlExtractor)),
        "application/json" => Route::Extract(Box::new(formats::text::JsonExtractor)),
        "text/csv" | "text/tab-separated-values" => {
            Route::Extract(Box::new(formats::text::CsvExtractor))
        }
        "application/sql" | "text/x-sql" => Route::Extract(Box::new(formats::text::SqlExtractor)),
        m if is_archive_mime(m) => Route::Extract(Box::new(formats::archive::ArchiveExtractor)),
        m if m.starts_with("image/") => Route::Extract(Box::new(formats::image::ImageExtractor)),
        m if m.starts_with("text/") => Route::Extract(Box::new(formats::text::PlaintextExtractor)),
        _ => Route::Skip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_mimes_are_recognized() {
        assert!(is_archive_mime("application/zip"));
        assert!(is_archive_mime("application/x-tar"));
        assert!(!is_archive_mime("text/plain"));
    }

    #[test]
    fn router_dispatches_supported_mimes() {
        // Routing must not panic for any input; bodies may be todo!() but construction is real.
        for mime in [
            "application/pdf",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "text/markdown",
            "text/html",
            "application/xml",
            "image/svg+xml",
            "application/json",
            "text/csv",
            "application/sql",
            "application/zip",
            "image/png",
            "text/x-rust",
            "text/plain",
        ] {
            assert!(
                matches!(router(mime), Route::Extract(_)),
                "{mime} should route to an extractor"
            );
        }
    }

    #[test]
    fn router_skips_mimes_with_no_extractor() {
        // Unidentifiable binary and anything without an extractor is skipped, not
        // lossily plaintext-decoded (PRD §4.8: octet-stream ⇒ status=5).
        for mime in [
            "application/octet-stream",
            "application/x-executable",
            "application/vnd.ms-excel",
            "video/mp4",
            "audio/mpeg",
            "font/woff2",
        ] {
            assert!(matches!(router(mime), Route::Skip), "{mime} should skip");
        }
    }
}
