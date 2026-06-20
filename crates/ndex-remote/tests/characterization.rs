//! Characterization tests for the public `ndex-remote` library interface.
//!
//! The wire<->engine mappings (`map`, `progress`) and the v0.2 stub helper are REAL. Command
//! handlers and the serve loop are `todo!()` and are exercised through the binary in
//! `tests/cli.rs`; here we pin the pure library surface and validate the clap definition.

use clap::{CommandFactory, Parser};
use ndex_core::progress::{ProgressChildUpdate, ProgressKind, ProgressUpdate};
use ndex_protocol::IndexOptions;
use ndex_reconcile::ReconcileStats;
use ndex_remote::cli::{Cli, Command};
use ndex_remote::commands::unavailable_v0_2;
use ndex_remote::map::{to_index_stats, to_reconcile_options};
use ndex_remote::progress::{phase_name, to_progress_event};

// ---------------------------------------------------------------------------
// Wire <-> engine mapping (server boundary).
// ---------------------------------------------------------------------------

#[test]
fn index_options_map_to_reconcile_options() {
    let wire = IndexOptions {
        full: true,
        verify: true,
        dry_run: true,
        jobs: Some(8),
        batch_size: Some(64),
        no_vectors: true,
        max_file_size: Some(1 << 30),
        only_new: true,
        ..Default::default()
    };
    let eng = to_reconcile_options(&wire);
    assert!(eng.full && eng.verify && eng.dry_run && eng.no_vectors && eng.only_new);
    assert_eq!(eng.jobs, Some(8));
    assert_eq!(eng.batch_size, Some(64));
    assert_eq!(eng.max_file_size, Some(1 << 30));
}

#[test]
fn reconcile_stats_map_to_index_stats() {
    let eng = ReconcileStats {
        new: 10,
        modified: 2,
        deleted: 1,
        unchanged: 7,
        processed: 9,
        failed: 1,
        skipped: 3,
        duration_ms: 42,
        timed_out: true,
    };
    let wire = to_index_stats(&eng);
    assert_eq!(wire.new, 10);
    assert_eq!(wire.modified, 2);
    assert_eq!(wire.deleted, 1);
    assert_eq!(wire.unchanged, 7);
    assert_eq!(wire.processed, 9);
    assert_eq!(wire.failed, 1);
    assert_eq!(wire.skipped, 3);
    assert_eq!(wire.duration_ms, 42);
    assert!(wire.timed_out);
}

// ---------------------------------------------------------------------------
// Progress bridging (PRD §13.7).
// ---------------------------------------------------------------------------

#[test]
fn phase_names_cover_every_kind() {
    assert_eq!(phase_name(ProgressKind::Walk), "walk");
    assert_eq!(phase_name(ProgressKind::Diff), "diff");
    assert_eq!(phase_name(ProgressKind::Extract), "extract");
    assert_eq!(phase_name(ProgressKind::Embed), "embed");
    assert_eq!(phase_name(ProgressKind::Fts), "fts");
    assert_eq!(phase_name(ProgressKind::Meta), "meta");
}

#[test]
fn progress_update_maps_to_event_with_children() {
    let update = ProgressUpdate {
        kind: ProgressKind::Extract,
        current: 100,
        total: Some(1000),
        message: Some("processing".into()),
        children: vec![ProgressChildUpdate {
            label: "worker-3".into(),
            current: 33,
            total: Some(250),
            message: None,
        }],
    };
    let event = to_progress_event(&update);
    assert_eq!(event.phase, "extract");
    assert_eq!(event.current, 100);
    assert_eq!(event.total, Some(1000));
    assert_eq!(event.children.len(), 1);
    assert_eq!(event.children[0].label, "worker-3");
    assert_eq!(event.children[0].current, 33);
}

// ---------------------------------------------------------------------------
// v0.2 stub helper.
// ---------------------------------------------------------------------------

#[test]
fn unavailable_v0_2_is_a_clear_error() {
    let err = unavailable_v0_2("tag").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("tag"));
    assert!(msg.contains("v0.2"));
}

// ---------------------------------------------------------------------------
// CLI definition (PRD §13.11).
// ---------------------------------------------------------------------------

#[test]
fn cli_definition_is_internally_consistent() {
    // clap's own invariant checker: panics if any arg/subcommand is malformed.
    Cli::command().debug_assert();
}

#[test]
fn v0_2_subcommands_parse_to_stub_variants() {
    assert!(matches!(
        Cli::try_parse_from(["ndex-remote", "tag"]).unwrap().command,
        Command::Tag
    ));
    assert!(matches!(
        Cli::try_parse_from(["ndex-remote", "dedup"])
            .unwrap()
            .command,
        Command::Dedup
    ));
    assert!(matches!(
        Cli::try_parse_from(["ndex-remote", "compact"])
            .unwrap()
            .command,
        Command::Compact
    ));
}

#[test]
fn serve_and_unknown_subcommands() {
    assert!(matches!(
        Cli::try_parse_from(["ndex-remote", "serve", "--root", "/pool"])
            .unwrap()
            .command,
        Command::Serve(_)
    ));
    assert!(Cli::try_parse_from(["ndex-remote", "does-not-exist"]).is_err());
}

#[test]
fn init_tracing_does_not_panic() {
    ndex_remote::init_tracing(2, false);
}
