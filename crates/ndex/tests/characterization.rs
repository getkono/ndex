//! Characterization tests for the public `ndex` (thin client) library interface.
//!
//! Target parsing, OSC 8 links, capability detection, and the `paths` renderer are REAL. The
//! pretty/plain/json/jsonl/csv renderers, transport, and session are `todo!()`; their contracts
//! are pinned by `#[ignore = "impl pending: PR #3"]` tests. Command handlers run the binary in
//! `tests/cli.rs`; here we validate the clap definition and the pure helpers.

use clap::{CommandFactory, Parser};
use ndex::args::{Cli, Command};
use ndex::commands::unavailable_v0_2;
use ndex::hosts::{Target, parse_target};
use ndex::render::{self, color, format, osc8};
use ndex_protocol::{OutputFormat, SearchResultData};

// ---------------------------------------------------------------------------
// Target parsing (PRD §13.2).
// ---------------------------------------------------------------------------

fn remote(host: &str, path: &str) -> Target {
    Target {
        host: Some(host.into()),
        path: path.into(),
    }
}
fn local(path: &str) -> Target {
    Target {
        host: None,
        path: path.into(),
    }
}

#[test]
fn parse_target_distinguishes_remote_from_local() {
    assert_eq!(
        parse_target("nas:/pool/archive"),
        remote("nas", "/pool/archive")
    );
    assert_eq!(
        parse_target("nas.local:/pool"),
        remote("nas.local", "/pool")
    );
    assert_eq!(parse_target("nas:"), remote("nas", "")); // alias, default root applied later
    assert_eq!(parse_target("/pool/archive"), local("/pool/archive"));
    assert_eq!(parse_target("rel/path"), local("rel/path"));
    // A colon that appears after a slash is part of the path, not a host separator.
    assert_eq!(parse_target("/pool:weird"), local("/pool:weird"));
    // Empty host before the colon ⇒ local.
    assert_eq!(parse_target(":x"), local(":x"));
}

// ---------------------------------------------------------------------------
// Rendering helpers (PRD §13.7, §14).
// ---------------------------------------------------------------------------

#[test]
fn osc8_wraps_uri_and_text() {
    assert_eq!(
        osc8("file:///pool/x.pdf", "x.pdf"),
        "\x1b]8;;file:///pool/x.pdf\x1b\\x.pdf\x1b]8;;\x1b\\"
    );
}

#[test]
fn color_scheme_is_ansi() {
    assert_eq!(color::RESET, "\x1b[0m");
    assert_eq!(color::PATH, "\x1b[1m");
    assert!(color::ERROR.starts_with('\x1b'));
}

#[test]
fn detect_caps_returns_sane_defaults() {
    let caps = render::detect_caps();
    // Even with no TTY, width/height fall back to a usable terminal size and unicode is on.
    assert!(caps.width >= 1);
    assert!(caps.height >= 1);
    assert!(caps.unicode);
}

#[test]
fn paths_renderer_emits_each_path() {
    // The `paths` format is the one renderer implemented in the skeleton; it must not panic and
    // returns Ok over zero or more hits.
    let empty = SearchResultData::default();
    format::paths(&empty).unwrap();
    let caps = render::detect_caps();
    render::render_search(&empty, OutputFormat::Paths, &caps).unwrap();
}

#[test]
#[ignore = "impl pending: PR #3"]
fn json_renderer_emits_a_json_object() {
    // Spec: `render_search` with Json produces a single JSON object (PRD §14).
    let caps = render::detect_caps();
    render::render_search(&SearchResultData::default(), OutputFormat::Json, &caps).unwrap();
}

// ---------------------------------------------------------------------------
// CLI definition + v0.2 stubs (PRD §13).
// ---------------------------------------------------------------------------

#[test]
fn cli_definition_is_internally_consistent() {
    Cli::command().debug_assert();
}

#[test]
fn search_and_v0_2_subcommands_parse() {
    assert!(matches!(
        Cli::try_parse_from(["ndex", "search", "needle", "/pool"])
            .unwrap()
            .command,
        Command::Search(_)
    ));
    assert!(matches!(
        Cli::try_parse_from(["ndex", "tag"]).unwrap().command,
        Command::Tag
    ));
    assert!(Cli::try_parse_from(["ndex", "bogus-subcommand"]).is_err());
}

#[test]
fn unavailable_v0_2_is_a_clear_error() {
    let err = unavailable_v0_2("dedup").unwrap_err();
    assert!(err.to_string().contains("v0.2"));
}

#[test]
fn init_tracing_does_not_panic() {
    ndex::init_tracing(1, false);
}
