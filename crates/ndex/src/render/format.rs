//! Per-format search-result renderers (PRD §13.2, §14).

use std::io::Write;

use ndex_core::error::Result;
use ndex_protocol::{SearchResultData, TerminalCaps};

/// Interactive `pretty` format: ranked, colorized, OSC 8 links, snippet highlights (PRD §14).
pub fn pretty(result: &SearchResultData, caps: &TerminalCaps) -> Result<()> {
    let _ = (result, caps);
    todo!()
}

/// `plain` format: the TTY-off default — no color/hyperlinks/progress (PRD §13.7).
pub fn plain(result: &SearchResultData) -> Result<()> {
    let _ = result;
    todo!()
}

/// `json` format: a single JSON object including `root` and raw scores (PRD §14).
pub fn json(result: &SearchResultData) -> Result<()> {
    let _ = result;
    todo!()
}

/// `jsonl` format: one JSON object per hit.
pub fn jsonl(result: &SearchResultData) -> Result<()> {
    let _ = result;
    todo!()
}

/// `paths` format: raw path bytes, one per line — for piping into `xargs` (PRD §13.7).
pub fn paths(result: &SearchResultData) -> Result<()> {
    let mut out = std::io::stdout().lock();
    for hit in &result.hits {
        out.write_all(hit.path.as_bytes())?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

/// `csv` format.
pub fn csv(result: &SearchResultData) -> Result<()> {
    let _ = result;
    todo!()
}
