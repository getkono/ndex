//! Characterization tests for the public `ndex-reconcile` interface.
//!
//! The pure classifiers (`classify_io_error`, `restat_unchanged`, `staleness`) and the
//! result value types are pinned directly. The three phases (`walk`/`diff`/`process`),
//! crash recovery, and the orchestrator are exercised end-to-end against a live `Store`
//! in a tempdir: full-reconcile idempotence, retry-then-promotion, skip dispositions
//! (unsupported MIME, `max_file_size`), `only_new`, dry-run purity, BLAKE3 persistence,
//! and symlink containment.

use std::io;
use std::time::Duration;

use ndex_core::config::Config;
use ndex_core::identity::{EmbeddingIdentity, FtsIdentity, Hashing, Identity, IndexIdentity};
use ndex_core::model::WalkEntry;
use ndex_core::path::NdexPath;
use ndex_core::progress::NullSink;
use ndex_core::status::FileStatus;
use ndex_reconcile::{
    DiffOutcome, ReconcileOptions, ReconcileStats, Reconciler, Staleness, WalkOutcome,
    classify_io_error, preflight_disk, preflight_memory, restat_unchanged, staleness, walk,
};
use ndex_store::Store;

// ---------------------------------------------------------------------------
// Test fixtures.
// ---------------------------------------------------------------------------

/// Create a fresh writable index at `root`.
fn test_store(root: &std::path::Path) -> Store {
    let identity = IndexIdentity {
        identity: Identity {
            schema_version: ndex_core::identity::SCHEMA_VERSION,
            created_by: "test".into(),
            created_at: "2026-06-19T00:00:00Z".into(),
        },
        embedding: EmbeddingIdentity {
            model_name: ndex_core::constants::DEFAULT_MODEL.into(),
            model_hash: "test".into(),
            dimensions: 768,
            mrl_dimensions: 256,
            vector_scalar: "f16".into(),
            hnsw_m: 32,
            hnsw_ef_construction: 200,
        },
        hashing: Hashing {
            algorithm: "blake3".into(),
        },
        fts: FtsIdentity {
            tokenizer_version: 1,
        },
    };
    Store::create(root, identity, Config::default()).unwrap()
}

/// Run one reconciliation with `options` over `store`.
fn reconcile(store: &mut Store, options: &ReconcileOptions) -> ReconcileStats {
    Reconciler::new(store, None)
        .run(options, &NullSink)
        .unwrap()
}

/// A `WalkEntry` built from the file's real on-disk metadata (same math as the walk).
fn entry_for(path: &std::path::Path) -> WalkEntry {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).unwrap();
    WalkEntry {
        size: meta.len(),
        mtime_ns: meta.mtime() * 1_000_000_000 + meta.mtime_nsec(),
        ctime_ns: meta.ctime() * 1_000_000_000 + meta.ctime_nsec(),
        inode: meta.ino(),
        dev: meta.dev(),
        mode: meta.mode(),
    }
}

fn npath(path: &std::path::Path) -> NdexPath {
    NdexPath::from_os_str(path.as_os_str())
}

fn count_progress_rows(store: &Store, file_id: i64) -> i64 {
    store
        .manifest
        .connection()
        .query_row(
            "SELECT count(*) FROM index_progress WHERE file_id = ?1",
            [file_id],
            |r| r.get(0),
        )
        .unwrap()
}

// ---------------------------------------------------------------------------
// I/O error classification (PRD §11.1, §11.5).
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
// Phase 1 — walk.
// ---------------------------------------------------------------------------

#[test]
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
fn walk_skips_symlinks_escaping_the_root_but_follows_within_root() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), b"outside content").unwrap();

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("real.txt"), b"inside content").unwrap();
    // Directory symlink escaping the root, and a file symlink resolving within it.
    std::os::unix::fs::symlink(outside.path(), tmp.path().join("escape")).unwrap();
    std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("inner")).unwrap();

    let config = Config::default();
    assert!(config.walk.follow_symlinks, "default follows symlinks");
    let outcome = walk(tmp.path(), &config).unwrap();
    let names: Vec<String> = outcome
        .files
        .iter()
        .map(|e| String::from_utf8_lossy(e.key().as_bytes()).into_owned())
        .collect();
    assert!(names.iter().any(|n| n.ends_with("real.txt")));
    assert!(
        names.iter().any(|n| n.ends_with("inner")),
        "within-root symlinks are still followed"
    );
    assert!(
        !names.iter().any(|n| n.contains("secret.txt")),
        "escaping symlink must not be followed: {names:?}"
    );
}

#[test]
fn preflight_memory_accepts_small_estimates() {
    // A handful of files must never trip the 75%-RAM abort.
    preflight_memory(10).unwrap();
}

#[test]
fn preflight_disk_accepts_small_estimates() {
    let tmp = tempfile::tempdir().unwrap();
    preflight_disk(tmp.path(), 1024).unwrap();
}

// ---------------------------------------------------------------------------
// End-to-end reconciliation.
// ---------------------------------------------------------------------------

#[test]
fn full_reconcile_indexes_a_tree() {
    // Create a fresh index at a source root, reconcile it, and report new/processed counts.
    // Exercises Store::create + Reconciler::run + walk/diff/process end to end.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"alpha bravo charlie").unwrap();
    std::fs::write(tmp.path().join("b.md"), b"# Notes\n\nsome words here").unwrap();

    let mut store = test_store(tmp.path());
    let stats = reconcile(&mut store, &ReconcileOptions::default());
    assert_eq!(stats.new, 2);
    assert_eq!(stats.processed, 2);
    assert_eq!(stats.failed, 0);

    // Indexed status and progress rows land only after the run-level FTS commit.
    let rec = store
        .manifest
        .get_by_path(&npath(&tmp.path().join("a.txt")))
        .unwrap()
        .unwrap();
    assert_eq!(rec.status, FileStatus::Indexed);
    assert_eq!(count_progress_rows(&store, rec.file_id), 1);

    // A second reconcile sees no changes.
    let again = reconcile(&mut store, &ReconcileOptions::default());
    assert_eq!(again.new, 0);
    assert_eq!(again.unchanged, 2);
    assert_eq!(again.processed, 0);
}

#[test]
fn blake3_hash_is_persisted_for_indexed_files() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("abc.txt"), b"abc").unwrap();

    let mut store = test_store(tmp.path());
    reconcile(&mut store, &ReconcileOptions::default());

    let rec = store
        .manifest
        .get_by_path(&npath(&tmp.path().join("abc.txt")))
        .unwrap()
        .unwrap();
    assert_eq!(rec.status, FileStatus::Indexed);
    assert_eq!(rec.blake3, Some(*blake3::hash(b"abc").as_bytes()));
}

#[test]
fn blake3_matches_official_known_vectors() {
    // Official BLAKE3 test vectors (b3sum of the empty input and of "abc").
    assert_eq!(
        blake3::hash(b"").to_hex().as_str(),
        "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
    );
    assert_eq!(
        blake3::hash(b"abc").to_hex().as_str(),
        "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
    );
}

// ---------------------------------------------------------------------------
// Retry / promotion policy (PRD §11.5).
// ---------------------------------------------------------------------------

#[test]
fn transient_failure_is_retried_when_under_the_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("flaky.txt");
    std::fs::write(&file, b"eventually readable").unwrap();

    let mut store = test_store(tmp.path());
    // Seed a manifest row matching the on-disk metadata with one transient failure.
    let id = store
        .manifest
        .upsert_walked(&npath(&file), &entry_for(&file))
        .unwrap();
    store
        .manifest
        .set_status(id, FileStatus::FailedTransient, Some("simulated I/O error"))
        .unwrap();

    // fail_count = 1 < max_retries (3): the run retries it despite unchanged metadata.
    let stats = reconcile(&mut store, &ReconcileOptions::default());
    assert_eq!(stats.modified, 1, "retry is classified for processing");
    assert_eq!(stats.processed, 1);
    let rec = store.manifest.get_by_path(&npath(&file)).unwrap().unwrap();
    assert_eq!(rec.status, FileStatus::Indexed);
    assert_eq!(rec.fail_count, 0, "success clears the failure streak");
    assert_eq!(rec.error_msg, None);
}

#[test]
fn exhausted_transient_failure_is_promoted_not_retried() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("hopeless.txt");
    std::fs::write(&file, b"never indexed").unwrap();

    let mut store = test_store(tmp.path());
    let id = store
        .manifest
        .upsert_walked(&npath(&file), &entry_for(&file))
        .unwrap();
    // max_retries (default 3) transient failures.
    for _ in 0..3 {
        store
            .manifest
            .set_status(id, FileStatus::FailedTransient, Some("simulated I/O error"))
            .unwrap();
    }

    let stats = reconcile(&mut store, &ReconcileOptions::default());
    assert_eq!(stats.processed, 0, "exhausted transients are not retried");
    assert_eq!(stats.failed, 0);
    assert_eq!(stats.modified, 0);
    assert_eq!(stats.unchanged, 1);
    let rec = store.manifest.get_by_path(&npath(&file)).unwrap().unwrap();
    assert_eq!(rec.status, FileStatus::FailedPermanent);
    assert_eq!(rec.fail_count, 3, "diagnostics preserved");
    assert_eq!(rec.error_msg.as_deref(), Some("simulated I/O error"));
}

// ---------------------------------------------------------------------------
// Skip dispositions (PRD §11.5: status=5, no FTS write, no progress row).
// ---------------------------------------------------------------------------

#[test]
fn unsupported_mime_is_skipped_with_status_5() {
    let tmp = tempfile::tempdir().unwrap();
    // NUL bytes + no magic + .bin extension ⇒ application/octet-stream ⇒ Route::Skip.
    std::fs::write(tmp.path().join("blob.bin"), b"\x00\x01\x02\x03binary junk").unwrap();

    let mut store = test_store(tmp.path());
    let stats = reconcile(&mut store, &ReconcileOptions::default());
    assert_eq!(stats.new, 1);
    assert_eq!(stats.skipped, 1);
    assert_eq!(stats.processed, 0);
    assert_eq!(stats.failed, 0);

    let rec = store
        .manifest
        .get_by_path(&npath(&tmp.path().join("blob.bin")))
        .unwrap()
        .unwrap();
    assert_eq!(rec.status, FileStatus::Skipped);
    assert_eq!(count_progress_rows(&store, rec.file_id), 0);

    // Skipped files stay untouched on the next run (no retry loop).
    let again = reconcile(&mut store, &ReconcileOptions::default());
    assert_eq!(again.unchanged, 1);
    assert_eq!(again.skipped, 0);
}

#[test]
fn oversized_files_are_skipped_without_reading() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("big.txt"), b"ten bytes!").unwrap();

    let mut store = test_store(tmp.path());
    let stats = reconcile(
        &mut store,
        &ReconcileOptions {
            max_file_size: Some(4), // options override config extraction.max_file_size
            ..ReconcileOptions::default()
        },
    );
    assert_eq!(stats.skipped, 1);
    assert_eq!(stats.processed, 0);

    let rec = store
        .manifest
        .get_by_path(&npath(&tmp.path().join("big.txt")))
        .unwrap()
        .unwrap();
    assert_eq!(rec.status, FileStatus::Skipped);
    assert!(
        rec.error_msg.unwrap().contains("max_file_size"),
        "skip reason is recorded"
    );
    assert_eq!(count_progress_rows(&store, rec.file_id), 0);

    // A Skipped file with unchanged metadata stays untouched on later runs...
    let again = reconcile(&mut store, &ReconcileOptions::default());
    assert_eq!(again.processed, 0);
    assert_eq!(again.unchanged, 1);

    // ...but once the file changes, the (default, 2 GiB) limit lets it index.
    std::fs::write(tmp.path().join("big.txt"), b"eleven bytes").unwrap();
    let stats = reconcile(&mut store, &ReconcileOptions::default());
    assert_eq!(stats.processed, 1);
}

// ---------------------------------------------------------------------------
// only_new (quick_reconcile contract).
// ---------------------------------------------------------------------------

#[test]
fn only_new_processes_new_files_but_not_modified_ones() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.txt");
    std::fs::write(&a, b"first version").unwrap();

    let mut store = test_store(tmp.path());
    reconcile(&mut store, &ReconcileOptions::default());
    let old_hash = store
        .manifest
        .get_by_path(&npath(&a))
        .unwrap()
        .unwrap()
        .blake3;

    // Modify a.txt and add b.txt; only_new must process b.txt only.
    std::fs::write(&a, b"second version, longer than before").unwrap();
    std::fs::write(tmp.path().join("b.txt"), b"brand new").unwrap();
    let stats = reconcile(
        &mut store,
        &ReconcileOptions {
            only_new: true,
            ..ReconcileOptions::default()
        },
    );
    assert_eq!(stats.new, 1);
    assert_eq!(stats.modified, 1, "the diff still reports the change");
    assert_eq!(stats.processed, 1, "but only the new file is processed");

    let rec = store.manifest.get_by_path(&npath(&a)).unwrap().unwrap();
    assert_eq!(rec.blake3, old_hash, "modified file was not reprocessed");

    // A full run picks the modification up.
    let full = reconcile(&mut store, &ReconcileOptions::default());
    assert_eq!(full.processed, 1);
    let rec = store.manifest.get_by_path(&npath(&a)).unwrap().unwrap();
    assert_ne!(rec.blake3, old_hash);
}

// ---------------------------------------------------------------------------
// Dry-run purity.
// ---------------------------------------------------------------------------

#[test]
fn dry_run_reports_but_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"alpha").unwrap();

    let mut store = test_store(tmp.path());
    let stats = reconcile(
        &mut store,
        &ReconcileOptions {
            dry_run: true,
            ..ReconcileOptions::default()
        },
    );
    assert_eq!(stats.new, 1);
    assert_eq!(stats.processed, 0);

    let conn = store.manifest.connection();
    let files: i64 = conn
        .query_row("SELECT count(*) FROM files", [], |r| r.get(0))
        .unwrap();
    assert_eq!(files, 0, "dry run must not upsert manifest rows");
    let runs: i64 = conn
        .query_row("SELECT count(*) FROM reconciliation_runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(runs, 0, "dry run must not record a run row");
    assert_eq!(store.manifest.last_reconciliation_ns().unwrap(), None);
}
