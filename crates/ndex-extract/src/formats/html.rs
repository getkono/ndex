//! HTML / XML / SVG extraction via `lol_html` (streaming) and `scraper` (DOM) (PRD §4.5, §4.8).
//!
//! Structure signals: `<h*>`, `<p>`, `<pre>`, `<li>`. `doc_meta` ← `<title>`, `<meta>`. XML and
//! SVG route here too: SVG text content (`<text>`/`<title>`/`<desc>`) is indexed and pixel
//! dimensions go to `media_meta` (PRD §4.8). HTML `<meta charset>` overrides encoding detection.

use ndex_core::error::Result;

use crate::extractor::{ExtractCtx, Extraction, Extractor};

/// Extracts text + metadata from HTML, XML, and SVG.
pub struct HtmlExtractor;

impl Extractor for HtmlExtractor {
    fn extract(&self, bytes: &[u8], ctx: &ExtractCtx<'_>) -> Result<Extraction> {
        let _ = (bytes, ctx);
        todo!()
    }
}
