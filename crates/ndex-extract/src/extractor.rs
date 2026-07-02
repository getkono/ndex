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

/// Route a MIME type to its extractor (PRD §4.4, §4.8).
///
/// Code files are routed here only when MIME detection yields a code type; the reconciler
/// also consults [`crate::mime::extension_language`] for extension/shebang-based routing.
pub fn router(mime: &str) -> Box<dyn Extractor> {
    match mime {
        "application/pdf" => Box::new(formats::pdf::PdfExtractor),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            Box::new(formats::docx::DocxExtractor)
        }
        "text/markdown" | "text/x-markdown" => Box::new(formats::markdown::MarkdownExtractor),
        "text/html"
        | "application/xhtml+xml"
        | "application/xml"
        | "text/xml"
        | "image/svg+xml" => Box::new(formats::html::HtmlExtractor),
        "application/json" => Box::new(formats::text::JsonExtractor),
        "text/csv" | "text/tab-separated-values" => Box::new(formats::text::CsvExtractor),
        "application/sql" | "text/x-sql" => Box::new(formats::text::SqlExtractor),
        m if is_archive_mime(m) => Box::new(formats::archive::ArchiveExtractor),
        m if m.starts_with("image/") => Box::new(formats::image::ImageExtractor),
        _ => Box::new(formats::text::PlaintextExtractor),
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
    fn router_covers_every_branch() {
        // Routing must not panic for any input; bodies are todo!() but construction is real.
        for mime in [
            "application/pdf",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "text/markdown",
            "text/html",
            "application/json",
            "text/csv",
            "application/sql",
            "application/zip",
            "image/png",
            "text/x-rust",
            "application/octet-stream",
        ] {
            let _ = router(mime);
        }
    }
}
