//! Read-family handlers: `search`, `info`, `stats`.

use ndex_core::SearchMode;
use ndex_core::error::{NdexError, Result};
use ndex_core::filters::SearchFilters;
use ndex_core::path::NdexPath;
use ndex_store::Store;

use crate::cli::{InfoArgs, PathArg, SearchArgs};

/// Parse the `--mode` string into a [`SearchMode`] (unknown ⇒ `auto`).
fn parse_mode(s: &str) -> SearchMode {
    match s {
        "fts" => SearchMode::Fts,
        "semantic" => SearchMode::Semantic,
        "hybrid" => SearchMode::Hybrid,
        _ => SearchMode::Auto,
    }
}

/// `ndex-remote search` (PRD §13.2).
pub fn search(args: SearchArgs) -> Result<()> {
    let store = Store::open(&args.path)?;
    let filters = SearchFilters::default();
    let outcome = ndex_search::run(
        &store,
        None,
        &args.query,
        parse_mode(&args.mode),
        &filters,
        args.limit as usize,
        args.offset as usize,
    )?;

    if outcome.hits.is_empty() {
        eprintln!("No results.");
        return Ok(());
    }

    let paths_only = args.format == "paths";
    for (i, hit) in outcome.hits.iter().enumerate() {
        let path = store.manifest.path_of(hit.file_id)?.map_or_else(
            || format!("file#{}", hit.file_id),
            |p| p.display_lossy().into_owned(),
        );
        if paths_only {
            println!("{path}");
            continue;
        }
        println!(
            "{:>3}. [{:.3}] {path}",
            args.offset as usize + i + 1,
            hit.score
        );
        if let Some(snippet) = store.fts.snippet(hit.file_id, hit.chunk_ord, &args.query)? {
            println!("     {}", render_snippet(&snippet));
        }
    }
    Ok(())
}

/// Render a tantivy snippet for the terminal: `<b>` highlights become bold, HTML entities are
/// unescaped.
fn render_snippet(snippet: &str) -> String {
    snippet
        .replace("<b>", "\x1b[1m")
        .replace("</b>", "\x1b[0m")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// `ndex-remote info` (PRD §13.5).
pub fn info(args: InfoArgs) -> Result<()> {
    let store = Store::open(&args.path)?;
    let target = NdexPath::from_os_str(args.file.as_os_str());
    let record = store
        .manifest
        .get_by_path(&target)?
        .ok_or_else(|| NdexError::Other(format!("not in index: {}", args.file.display())))?;
    println!("path:    {}", record.path.display_lossy());
    println!("file_id: {}", record.file_id);
    println!("size:    {} bytes", record.size);
    println!("status:  {:?}", record.status);
    println!("mime:    {}", record.mime_type.as_deref().unwrap_or("-"));
    Ok(())
}

/// `ndex-remote stats` (PRD §13.5).
pub fn stats(args: PathArg) -> Result<()> {
    let store = Store::open(&args.path)?;
    let files = store.manifest.live_files()?.len();
    println!("index:  {}/.ndex", args.path.display());
    println!("model:  {}", store.identity.embedding.model_name);
    println!("files:  {files}");
    match store.manifest.last_reconciliation_ns()? {
        Some(ns) => println!("last reconcile: {ns} ns"),
        None => println!("last reconcile: never"),
    }
    Ok(())
}
