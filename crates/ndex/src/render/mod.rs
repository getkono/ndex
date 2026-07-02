//! Terminal rendering of results (PRD §13.7, §14).

pub mod format;

use ndex_core::error::Result;
use ndex_protocol::{OutputFormat, SearchResultData, TerminalCaps};

/// Semantic ANSI color scheme (PRD §13.7).
pub mod color {
    pub const PATH: &str = "\x1b[1m";
    pub const MATCH: &str = "\x1b[1;33m";
    pub const SCORE: &str = "\x1b[2m";
    pub const MIME: &str = "\x1b[36m";
    pub const SIZE: &str = "\x1b[32m";
    pub const DATE: &str = "\x1b[34m";
    pub const ERROR: &str = "\x1b[31m";
    pub const TAG: &str = "\x1b[35m";
    pub const RESET: &str = "\x1b[0m";
}

/// Format an OSC 8 hyperlink (PRD §13.7).
pub fn osc8(uri: &str, text: &str) -> String {
    format!("\x1b]8;;{uri}\x1b\\{text}\x1b]8;;\x1b\\")
}

/// Detect terminal capabilities to advertise in the handshake (PRD §12.7).
pub fn detect_caps() -> TerminalCaps {
    let (width, height) = terminal_size::terminal_size().map_or((80, 24), |(w, h)| (w.0, h.0));
    let color = supports_color::on(supports_color::Stream::Stdout).is_some();
    let hyperlinks = supports_hyperlinks::on(supports_hyperlinks::Stream::Stdout);
    TerminalCaps {
        width,
        height,
        color,
        hyperlinks,
        unicode: true,
    }
}

/// Render a search result in the requested format (PRD §14). When stdout is not a TTY the caller
/// downgrades `Pretty` → `Plain` and disables color/hyperlinks (PRD §13.7).
pub fn render_search(
    result: &SearchResultData,
    fmt: OutputFormat,
    caps: &TerminalCaps,
) -> Result<()> {
    match fmt {
        OutputFormat::Pretty => format::pretty(result, caps),
        OutputFormat::Plain => format::plain(result),
        OutputFormat::Json => format::json(result),
        OutputFormat::Jsonl => format::jsonl(result),
        OutputFormat::Paths => format::paths(result),
        OutputFormat::Csv => format::csv(result),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc8_wraps_uri_and_text() {
        let link = osc8("file:///pool/x.pdf", "x.pdf");
        assert_eq!(link, "\x1b]8;;file:///pool/x.pdf\x1b\\x.pdf\x1b]8;;\x1b\\");
    }
}
