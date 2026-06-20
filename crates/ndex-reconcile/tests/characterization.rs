//! Characterization tests for the public `ndex-reconcile` interface.
//!
//! The pure classifiers (`classify_io_error`, `restat_unchanged`, `staleness`) and the result
//! value types are REAL. The three phases (`walk`/`diff`/`process`), recovery, and the
//! orchestrator need a live `Store`/`Manifest` (also `todo!()`), so their contracts are pinned by
//! `#[ignore = "impl pending: PR #3"]` tests that compile against the real signatures.

use std::io;
use std::time::Duration;

use ndex_core::config::Config;
use ndex_core::model::WalkEntry;
use ndex_core::status::FileStatus;
use ndex_reconcile::{
    DiffOutcome, ReconcileOptions, ReconcileStats, Staleness, WalkOutcome, classify_io_error,
    preflight_disk, preflight_memory, restat_unchanged, staleness, walk,
};

// ---------------------------------------------------------------------------
// I/O error classification (PRD §11.1).
// ---------------------------------------------------------------------------

#[test]
fn enoent_means_deleted_everything_else_is_transient() {
    assert_eq!(
        classify_io_error(&io::Error::from(io::ErrorKind::NotFound)),
        FileStatus::Deleted
    );
    for kind in [
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::Other,
        io::ErrorKind::TimedOut,
        io::ErrorKind::Interrupted,
    ] {
        assert_eq!(
            classify_io_error(&io::Error::from(kind)),
            FileStatus::FailedTransient,
            "{kind:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// TOCTOU restat guard (PRD §11.1).
// ---------------------------------------------------------------------------

#[test]
fn restat_requires_both_size_and_mtime_unchanged() {
    let walked = WalkEntry {
        size: 100,
        mtime_ns: 5,
        ctime_ns: 0,
        inode: 1,
        dev: 1,
        mode: 0o644,
    };
    assert!(restat_unchanged(&walked, 100, 5));
    assert!(!restat_unchanged(&walked, 101, 5)); // size changed
    assert!(!restat_unchanged(&walked, 100, 6)); // mtime changed
    assert!(!restat_unchanged(&walked, 0, 0)); // both changed
}

// ---------------------------------------------------------------------------
// Staleness classification (PRD §6.2).
// ---------------------------------------------------------------------------

#[test]
fn staleness_boundaries() {
    let threshold = Duration::from_secs(3_600); // 1h
    let warn = Duration::from_secs(604_800); // 7d
    let hour_ns: i64 = 3_600_000_000_000;
    let now = 1_000_000_000_000_000;

    // Never reconciled ⇒ Warn.
    assert_eq!(staleness(None, now, threshold, warn), Staleness::Warn);
    // Just reconciled ⇒ Fresh.
    assert_eq!(staleness(Some(now), now, threshold, warn), Staleness::Fresh);
    // Exactly at the threshold ⇒ Stale (Fresh is strictly younger).
    assert_eq!(
        staleness(Some(now - hour_ns), now, threshold, warn),
        Staleness::Stale
    );
    // Between thresholds ⇒ Stale.
    assert_eq!(
        staleness(Some(now - 2 * hour_ns), now, threshold, warn),
        Staleness::Stale
    );
    // Past the warn threshold ⇒ Warn.
    assert_eq!(
        staleness(Some(now - 8 * 24 * hour_ns), now, threshold, warn),
        Staleness::Warn
    );
    // Clock skew (last in the future) is clamped to age 0 ⇒ Fresh.
    assert_eq!(
        staleness(Some(now + hour_ns), now, threshold, warn),
        Staleness::Fresh
    );
}

// ---------------------------------------------------------------------------
// Result value types.
// ---------------------------------------------------------------------------

#[test]
fn reconcile_options_default_is_inert() {
    let o = ReconcileOptions::default();
    assert!(!o.full && !o.verify && !o.dry_run && !o.no_vectors && !o.only_new);
    assert!(o.jobs.is_none() && o.batch_size.is_none() && o.max_file_size.is_none());
}

#[test]
fn reconcile_stats_default_is_zeroed() {
    let s = ReconcileStats::default();
    assert_eq!(s, ReconcileStats::default());
    assert_eq!(
        s.new + s.modified + s.deleted + s.unchanged + s.processed,
        0
    );
    assert!(!s.timed_out);
}

#[test]
fn walk_and_diff_outcomes_default_empty() {
    let w = WalkOutcome::default();
    assert!(w.files.is_empty() && w.dirs.is_empty());
    let d = DiffOutcome::default();
    assert!(d.new.is_empty() && d.modified.is_empty() && d.deleted.is_empty());
    assert_eq!(d.unchanged, 0);
}

// ---------------------------------------------------------------------------
// todo!() contracts (PR #3 targets).
// ---------------------------------------------------------------------------

#[test]
#[ignore = "impl pending: PR #3"]
fn walk_collects_regular_files_and_honors_ignores() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("keep.txt"), b"hello").unwrap();
    std::fs::write(tmp.path().join("skip.log"), b"noise").unwrap();
    std::fs::write(tmp.path().join(".ndexignore"), b"*.log\n").unwrap();

    let outcome = walk(tmp.path(), &Config::default()).unwrap();
    let names: Vec<String> = outcome
        .files
        .iter()
        .map(|e| String::from_utf8_lossy(e.key().as_bytes()).into_owned())
        .collect();
    assert!(names.iter().any(|n| n.ends_with("keep.txt")));
    assert!(!names.iter().any(|n| n.ends_with("skip.log")));
}

#[test]
#[ignore = "impl pending: PR #3"]
fn preflight_memory_accepts_small_estimates() {
    // A handful of files must never trip the 75%-RAM abort.
    preflight_memory(10).unwrap();
}

#[test]
#[ignore = "impl pending: PR #3"]
fn preflight_disk_accepts_small_estimates() {
    let tmp = tempfile::tempdir().unwrap();
    preflight_disk(tmp.path(), 1024).unwrap();
}

#[test]
#[ignore = "impl pending: PR #3"]
fn full_reconcile_indexes_a_tree() {
    // Spec: open a store at a fresh root, reconcile a source tree, and report new-file counts.
    // Exercises Store::create + Reconciler::run + walk/diff/process end to end.
    use ndex_core::progress::NullSink;
    use ndex_reconcile::Reconciler;

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"alpha").unwrap();
    let mut store = ndex_store::Store::open(tmp.path()).unwrap();
    let mut rec = Reconciler::new(&mut store, None);
    let stats = rec.run(&ReconcileOptions::default(), &NullSink).unwrap();
    assert_eq!(stats.new, 1);
}
