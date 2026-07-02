//! Characterization tests for the public `ndex-store` interface.
//!
//! The schema DDL and the sidecar header constant are pinned by executing the DDL in SQLite and
//! asserting tables/columns/indexes/pragmas. The engine I/O (manifest/meta CRUD, tantivy, lock,
//! `Store`) is implemented and exercised directly. Only the usearch vector index remains
//! unimplemented; its intended contract is pinned by `#[ignore = "impl pending: PR #3"]` tests
//! that compile against the real signatures.

use ndex_core::model::{DocMeta, WalkEntry};
use ndex_core::path::NdexPath;
use ndex_core::status::FileStatus;
use ndex_store::manifest::{Change, MANIFEST_PRAGMAS, MANIFEST_SCHEMA, Manifest, RunKind};
use ndex_store::meta::{META_PRAGMAS, META_SCHEMA, MetaDb};
use ndex_store::vector::{SIDECAR_MAGIC, Sidecar, VectorIndex};

// ---------------------------------------------------------------------------
// Schema DDL — executed live in an in-memory database.
// ---------------------------------------------------------------------------

fn mem(schema: &str) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(schema).expect("schema executes");
    conn
}

fn has_table(conn: &rusqlite::Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        == 1
}

fn has_column(conn: &rusqlite::Connection, table: &str, col: &str) -> bool {
    conn.query_row(
        &format!("SELECT count(*) FROM pragma_table_info('{table}') WHERE name=?1"),
        [col],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        == 1
}

fn has_index(conn: &rusqlite::Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='index' AND name=?1",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        == 1
}

#[test]
fn manifest_schema_tables_columns_indexes() {
    let conn = mem(MANIFEST_SCHEMA);
    for t in [
        "files",
        "index_progress",
        "reconciliation_runs",
        "schema_info",
    ] {
        assert!(has_table(&conn, t), "missing table {t}");
    }
    for c in [
        "file_id",
        "path",
        "path_hash",
        "size",
        "mtime_ns",
        "ctime_ns",
        "mode",
        "blake3",
        "mime_type",
        "status",
        "fail_count",
        "first_seen_ns",
        "last_verified_ns",
        "hard_link_of",
        "parent_archive_id",
    ] {
        assert!(has_column(&conn, "files", c), "files missing column {c}");
    }
    for idx in [
        "idx_path",
        "idx_path_hash",
        "idx_status",
        "idx_blake3",
        "idx_mtime",
        "idx_mime",
        "idx_size",
        "idx_hard_link",
        "idx_parent_archive",
    ] {
        assert!(has_index(&conn, idx), "missing index {idx}");
    }
}

#[test]
fn meta_schema_tables_and_lens_column() {
    let conn = mem(META_SCHEMA);
    for t in [
        "doc_meta",
        "media_meta",
        "tags",
        "file_tags",
        "archive_meta",
    ] {
        assert!(has_table(&conn, t), "missing table {t}");
    }
    // PRD reconciliation: media_meta carries `lens` (matches the wire MediaMeta).
    assert!(has_column(&conn, "media_meta", "lens"));
    assert!(has_column(&conn, "doc_meta", "page_count"));
    assert!(has_column(&conn, "archive_meta", "extraction_status"));
}

#[test]
fn pragmas_enable_wal() {
    for pragmas in [MANIFEST_PRAGMAS, META_PRAGMAS] {
        assert!(pragmas.contains("journal_mode"));
        assert!(pragmas.contains("WAL"));
        assert!(pragmas.contains("foreign_keys"));
    }
}

#[test]
fn pragmas_effective_on_disk() {
    // String greps above don't prove anything sticks; assert against real disk-backed dbs.
    let tmp = tempfile::tempdir().unwrap();
    let manifest = Manifest::open_or_create(&tmp.path().join("manifest.db")).unwrap();
    let meta = MetaDb::open_or_create(&tmp.path().join("meta.db")).unwrap();
    for conn in [manifest.connection(), meta.connection()] {
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }
}

#[test]
fn foreign_keys_are_enforced_and_cascade() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = Manifest::open_or_create(&tmp.path().join("manifest.db")).unwrap();
    let path = NdexPath::new(b"/pool/fk.txt".to_vec());
    let id = manifest.upsert_walked(&path, &walk_entry()).unwrap();
    manifest.record_progress(id, "fts", 1).unwrap();

    // Orphan index_progress rows are rejected...
    let orphan = manifest.connection().execute(
        "INSERT INTO index_progress (file_id, index_name, schema_ver, indexed_at_ns) \
         VALUES (?1, 'fts', 1, 0)",
        [id + 1000],
    );
    assert!(orphan.is_err(), "orphan insert must violate the FK");

    // ...and deleting the file cascades to its progress rows (ON DELETE CASCADE is live).
    manifest
        .connection()
        .execute("DELETE FROM files WHERE file_id = ?1", [id])
        .unwrap();
    let left: i64 = manifest
        .connection()
        .query_row(
            "SELECT count(*) FROM index_progress WHERE file_id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn run_kind_and_change_are_distinct() {
    assert_ne!(RunKind::Full, RunKind::Incremental);
    assert_ne!(Change::New, Change::Unchanged);
    assert_eq!(Change::Deleted, Change::Deleted);
}

// ---------------------------------------------------------------------------
// Vector sidecar header (PRD §10.3) — real constant + empty-state.
// ---------------------------------------------------------------------------

#[test]
fn sidecar_magic_is_distinct_and_fixed_width() {
    assert_eq!(SIDECAR_MAGIC, b"NDEXVEC\0");
    assert_eq!(SIDECAR_MAGIC.len(), 8);
    // Must NOT collide with the IPC preamble.
    assert_ne!(&SIDECAR_MAGIC[..], ndex_core::constants::MAGIC_PREAMBLE);
}

#[test]
fn empty_sidecar_is_empty() {
    let s = Sidecar::new();
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);
}

// ---------------------------------------------------------------------------
// Engine I/O behavior (manifest/meta CRUD, tantivy, lock, Store).
// ---------------------------------------------------------------------------

fn walk_entry() -> WalkEntry {
    WalkEntry {
        size: 1024,
        mtime_ns: 5,
        ctime_ns: 6,
        inode: 1,
        dev: 1,
        mode: 0o100644,
    }
}

#[test]
fn manifest_upsert_classify_and_status_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = Manifest::open_or_create(&tmp.path().join("manifest.db")).unwrap();
    let path = NdexPath::new(b"/pool/a.txt".to_vec());
    let entry = walk_entry();

    let id = manifest.upsert_walked(&path, &entry).unwrap();
    assert!(id > 0);
    let rec = manifest.get_by_path(&path).unwrap().unwrap();
    assert_eq!(rec.status, FileStatus::Pending);

    // A Pending row with unchanged metadata is a Retry (interrupted run), not Unchanged.
    assert_eq!(manifest.classify(&path, &entry, 3).unwrap(), Change::Retry);

    manifest.set_status(id, FileStatus::Indexed, None).unwrap();
    assert_eq!(
        manifest.classify(&path, &entry, 3).unwrap(),
        Change::Unchanged
    );

    manifest.touch_last_reconciliation(42).unwrap();
    assert_eq!(manifest.last_reconciliation_ns().unwrap(), Some(42));

    let run = manifest.begin_run(RunKind::Incremental, "mtime").unwrap();
    manifest.finish_run(run).unwrap();
}

#[test]
fn upsert_walked_resets_status_only_when_size_or_mtime_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = Manifest::open_or_create(&tmp.path().join("manifest.db")).unwrap();
    let path = NdexPath::new(b"/pool/reset.txt".to_vec());
    let entry = walk_entry();

    let id = manifest.upsert_walked(&path, &entry).unwrap();
    manifest
        .set_status(id, FileStatus::FailedTransient, Some("boom"))
        .unwrap();

    // Unchanged (size, mtime_ns): status, fail_count, and error_msg are preserved.
    let same = manifest.upsert_walked(&path, &entry).unwrap();
    assert_eq!(same, id);
    let rec = manifest.get_by_path(&path).unwrap().unwrap();
    assert_eq!(rec.status, FileStatus::FailedTransient);
    assert_eq!(rec.fail_count, 1);
    assert_eq!(rec.error_msg.as_deref(), Some("boom"));

    // Changed content: status resets to Pending and retry accounting is cleared.
    let changed = WalkEntry {
        size: entry.size + 1,
        ..entry
    };
    manifest.upsert_walked(&path, &changed).unwrap();
    let rec = manifest.get_by_path(&path).unwrap().unwrap();
    assert_eq!(rec.status, FileStatus::Pending);
    assert_eq!(rec.fail_count, 0);
    assert_eq!(rec.error_msg, None);
}

#[test]
fn mark_indexed_flips_status_persists_blake3_and_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = Manifest::open_or_create(&tmp.path().join("manifest.db")).unwrap();
    let path = NdexPath::new(b"/pool/batch.txt".to_vec());
    let id = manifest.upsert_walked(&path, &walk_entry()).unwrap();
    manifest
        .set_status(id, FileStatus::FailedTransient, Some("old error"))
        .unwrap();

    let hash = [7u8; 32];
    manifest.mark_indexed(&[(id, hash)], "fts", 1).unwrap();
    let rec = manifest.get_by_path(&path).unwrap().unwrap();
    assert_eq!(rec.status, FileStatus::Indexed);
    assert_eq!(rec.blake3, Some(hash));
    assert_eq!(rec.fail_count, 0, "success clears the failure streak");
    assert_eq!(rec.error_msg, None);
    let progress: i64 = manifest
        .connection()
        .query_row(
            "SELECT count(*) FROM index_progress WHERE file_id = ?1 AND index_name = 'fts'",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(progress, 1);

    // Recovery: a non-Indexed/Skipped status with a progress row is a candidate.
    assert!(manifest.recovery_candidates().unwrap().is_empty());
    manifest.set_status(id, FileStatus::Pending, None).unwrap();
    assert_eq!(manifest.recovery_candidates().unwrap(), vec![id]);
    manifest.clear_progress(id).unwrap();
    assert!(manifest.recovery_candidates().unwrap().is_empty());
}

#[test]
fn exhausted_transients_promote_to_permanent() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = Manifest::open_or_create(&tmp.path().join("manifest.db")).unwrap();
    let entry = walk_entry();
    let under = NdexPath::new(b"/pool/under.txt".to_vec());
    let over = NdexPath::new(b"/pool/over.txt".to_vec());
    let under_id = manifest.upsert_walked(&under, &entry).unwrap();
    let over_id = manifest.upsert_walked(&over, &entry).unwrap();
    manifest
        .set_status(under_id, FileStatus::FailedTransient, Some("once"))
        .unwrap();
    for _ in 0..3 {
        manifest
            .set_status(over_id, FileStatus::FailedTransient, Some("thrice"))
            .unwrap();
    }

    assert_eq!(manifest.promote_exhausted_transients(3).unwrap(), 1);
    let under_rec = manifest.get_by_path(&under).unwrap().unwrap();
    assert_eq!(under_rec.status, FileStatus::FailedTransient);
    assert_eq!(manifest.classify(&under, &entry, 3).unwrap(), Change::Retry);
    let over_rec = manifest.get_by_path(&over).unwrap().unwrap();
    assert_eq!(over_rec.status, FileStatus::FailedPermanent);
    assert_eq!(over_rec.fail_count, 3, "diagnostics preserved");
    assert_eq!(over_rec.error_msg.as_deref(), Some("thrice"));
    assert_eq!(
        manifest.classify(&over, &entry, 3).unwrap(),
        Change::Unchanged
    );
}

#[test]
fn meta_db_roundtrips_doc_meta() {
    let tmp = tempfile::tempdir().unwrap();
    let meta = MetaDb::open_or_create(&tmp.path().join("meta.db")).unwrap();
    let doc = DocMeta {
        title: Some("Q3".into()),
        page_count: Some(10),
        ..DocMeta::default()
    };
    meta.upsert_doc_meta(1, &doc).unwrap();
    assert_eq!(meta.doc_meta(1).unwrap(), Some(doc));
    meta.delete_file(1).unwrap();
    assert_eq!(meta.doc_meta(1).unwrap(), None);
}

#[test]
#[ignore = "impl pending: PR #3"]
fn vector_index_add_search_save_load() {
    use half::f16;
    let tmp = tempfile::tempdir().unwrap();
    let mut idx = VectorIndex::open_or_create(tmp.path(), 256).unwrap();
    let v: Vec<f16> = (0..256).map(|i| f16::from_f32(i as f32 / 256.0)).collect();
    idx.add(1, 0, &v).unwrap();
    assert_eq!(idx.len(), 1);
    let hits = idx.search(&v, 5).unwrap();
    assert_eq!(hits.first().map(|h| h.file_id), Some(1));
    idx.save(tmp.path()).unwrap();
    let reloaded = VectorIndex::load_and_validate(tmp.path()).unwrap();
    assert_eq!(reloaded.len(), 1);
}

fn chunk(chunk_ord: u32, text: &str) -> ndex_core::model::Chunk {
    ndex_core::model::Chunk {
        file_id: 0,
        chunk_ord,
        byte_start: 0,
        byte_end: text.len() as u64,
        block_type: ndex_core::model::BlockType::Paragraph,
        text: text.into(),
    }
}

fn fts_meta<'a>(path_text: &'a str, title: Option<&'a str>) -> ndex_store::fts::FtsFileMeta<'a> {
    ndex_store::fts::FtsFileMeta {
        mime: "text/plain",
        lang: Some("eng"),
        path_text,
        size: 1024,
        mtime_ns: 1_700_000_000_000_000_000,
        title,
    }
}

#[test]
fn fts_index_add_commit_search() {
    use ndex_store::fts::FtsIndex;
    // Schema must declare the core retrieval fields.
    let schema = FtsIndex::build_schema();
    assert!(schema.get_field("body").is_ok());
    assert!(schema.get_field("file_id").is_ok());

    let tmp = tempfile::tempdir().unwrap();
    let mut fts = FtsIndex::open_or_create(tmp.path()).unwrap();
    fts.add_chunk(
        1,
        &chunk(0, "hello world"),
        &fts_meta("notes/hello.txt", Some("Greetings")),
    )
    .unwrap();
    fts.commit().unwrap();
    let hits = fts.search("hello", 10, 2.0).unwrap();
    assert_eq!(hits.first().map(|h| h.file_id), Some(1));
    assert!(fts.snippet(1, 0, "hello").unwrap().is_some());
}

#[test]
fn fts_title_and_path_text_are_indexed() {
    use ndex_store::fts::FtsIndex;
    let tmp = tempfile::tempdir().unwrap();
    let mut fts = FtsIndex::open_or_create(tmp.path()).unwrap();
    fts.add_chunk(
        1,
        &chunk(0, "alpha beta"),
        &fts_meta("docs/q3-report.txt", Some("quarterly earnings")),
    )
    .unwrap();
    fts.add_chunk(
        2,
        &chunk(0, "gamma delta"),
        &fts_meta("misc/other.txt", None),
    )
    .unwrap();
    fts.commit().unwrap();

    // `title` is a default query field: a term found only in the title matches.
    let hits = fts.search("quarterly", 10, 2.0).unwrap();
    assert_eq!(hits.iter().map(|h| h.file_id).collect::<Vec<_>>(), vec![1]);

    // `path_text` is indexed and reachable via explicit field syntax.
    let hits = fts.search("path_text:report", 10, 2.0).unwrap();
    assert_eq!(hits.iter().map(|h| h.file_id).collect::<Vec<_>>(), vec![1]);
}

#[test]
fn fts_delete_file_purges_all_chunks() {
    use ndex_store::fts::FtsIndex;
    let tmp = tempfile::tempdir().unwrap();
    let mut fts = FtsIndex::open_or_create(tmp.path()).unwrap();
    for ord in 0..3 {
        fts.add_chunk(1, &chunk(ord, "needle one"), &fts_meta("a.txt", None))
            .unwrap();
    }
    fts.add_chunk(2, &chunk(0, "needle two"), &fts_meta("b.txt", None))
        .unwrap();
    fts.commit().unwrap();
    assert_eq!(fts.search("needle", 10, 2.0).unwrap().len(), 4);

    fts.delete_file(1).unwrap();
    fts.commit().unwrap();
    let hits = fts.search("needle", 10, 2.0).unwrap();
    assert_eq!(hits.iter().map(|h| h.file_id).collect::<Vec<_>>(), vec![2]);
}

#[test]
fn fts_search_with_total_reports_true_count() {
    use ndex_store::fts::FtsIndex;
    let tmp = tempfile::tempdir().unwrap();
    let mut fts = FtsIndex::open_or_create(tmp.path()).unwrap();
    for id in 1..=5 {
        fts.add_chunk(
            id,
            &chunk(0, "needle in haystack"),
            &fts_meta("f.txt", None),
        )
        .unwrap();
    }
    fts.commit().unwrap();

    let (hits, total) = fts.search_with_total("needle", 2, 2.0).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(total, 5);
    // `search` delegates and returns the same window.
    assert_eq!(fts.search("needle", 2, 2.0).unwrap(), hits);
}

#[test]
fn fts_readonly_searches_but_rejects_writes() {
    use ndex_store::fts::FtsIndex;
    let tmp = tempfile::tempdir().unwrap();
    let mut fts = FtsIndex::open_or_create(tmp.path()).unwrap();
    fts.add_chunk(1, &chunk(0, "hello world"), &fts_meta("a.txt", None))
        .unwrap();
    fts.commit().unwrap();
    drop(fts);

    // Two read-only handles coexist (no tantivy writer lock is taken).
    let mut ro1 = FtsIndex::open_readonly(tmp.path()).unwrap();
    let ro2 = FtsIndex::open_readonly(tmp.path()).unwrap();
    assert_eq!(ro1.search("hello", 10, 2.0).unwrap().len(), 1);
    assert_eq!(ro2.search("hello", 10, 2.0).unwrap().len(), 1);

    // All write paths fail without a writer.
    assert!(
        ro1.add_chunk(2, &chunk(0, "x"), &fts_meta("b.txt", None))
            .is_err()
    );
    assert!(ro1.delete_file(1).is_err());
    assert!(ro1.commit().is_err());
}

#[test]
fn lock_is_exclusive() {
    let tmp = tempfile::tempdir().unwrap();
    let _held = ndex_store::lock::IndexLock::acquire(tmp.path()).unwrap();
    // A second non-blocking attempt sees the lock as held.
    assert!(
        ndex_store::lock::IndexLock::try_acquire(tmp.path())
            .unwrap()
            .is_none()
    );
}

#[test]
fn shared_locks_coexist() {
    use ndex_store::lock::IndexLock;
    let tmp = tempfile::tempdir().unwrap();
    let a = IndexLock::acquire_shared(tmp.path()).unwrap();
    // A second reader enters freely (blocking and non-blocking variants).
    let b = IndexLock::try_acquire_shared(tmp.path()).unwrap();
    assert!(b.is_some());
    let _c = IndexLock::acquire_shared(tmp.path()).unwrap();
    // A writer cannot enter while readers hold the lock.
    assert!(IndexLock::try_acquire(tmp.path()).unwrap().is_none());
    drop((a, b));
}

#[test]
fn exclusive_excludes_shared() {
    use ndex_store::lock::IndexLock;
    let tmp = tempfile::tempdir().unwrap();
    let held = IndexLock::acquire(tmp.path()).unwrap();
    assert!(IndexLock::try_acquire_shared(tmp.path()).unwrap().is_none());
    drop(held);
    assert!(IndexLock::try_acquire_shared(tmp.path()).unwrap().is_some());
}

/// Helper for `exclusive_lock_contends_across_processes`: when re-spawned with
/// `NDEX_LOCK_TEST_DIR` set, holds the exclusive lock until the parent writes the
/// `release` marker. In a normal test run (env absent) it does nothing and passes.
#[test]
fn cross_process_lock_helper() {
    let Ok(dir) = std::env::var("NDEX_LOCK_TEST_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let _lock = ndex_store::lock::IndexLock::acquire(&dir).unwrap();
    std::fs::write(dir.join("locked"), b"").unwrap();
    let release = dir.join("release");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !release.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "parent never wrote the release marker"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn exclusive_lock_contends_across_processes() {
    use ndex_store::lock::IndexLock;
    let tmp = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["cross_process_lock_helper", "--exact", "--nocapture"])
        .env("NDEX_LOCK_TEST_DIR", tmp.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    // Wait (bounded) until the child reports it holds the exclusive lock.
    let locked = tmp.path().join("locked");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !locked.exists() {
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            panic!("child never acquired the lock");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // Another process's exclusive lock blocks both shared and exclusive attempts here.
    assert!(IndexLock::try_acquire_shared(tmp.path()).unwrap().is_none());
    assert!(IndexLock::try_acquire(tmp.path()).unwrap().is_none());

    std::fs::write(tmp.path().join("release"), b"").unwrap();
    assert!(child.wait().unwrap().success());

    // After the child exits, the lock is free again.
    assert!(IndexLock::try_acquire_shared(tmp.path()).unwrap().is_some());
}

fn test_identity() -> ndex_core::identity::IndexIdentity {
    use ndex_core::identity::{
        EmbeddingIdentity, FtsIdentity, Hashing, Identity, IndexIdentity, SCHEMA_VERSION,
    };
    IndexIdentity {
        identity: Identity {
            schema_version: SCHEMA_VERSION,
            created_by: "test".into(),
            created_at: "2026-06-19T00:00:00Z".into(),
        },
        embedding: EmbeddingIdentity {
            model_name: ndex_core::constants::DEFAULT_MODEL.into(),
            model_hash: "abc".into(),
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
    }
}

#[test]
#[ignore = "impl pending: PR #3"]
fn store_create_then_open_roundtrips() {
    use ndex_core::config::Config;
    use ndex_store::Store;

    let tmp = tempfile::tempdir().unwrap();
    let identity = test_identity();
    let created = Store::create(tmp.path(), identity.clone(), Config::default()).unwrap();
    assert!(created.vectors.is_some());
    drop(created);
    let opened = Store::open(tmp.path()).unwrap();
    assert_eq!(opened.identity, identity);
    let _lock = opened.lock();
}

#[test]
fn store_open_read_allows_concurrent_readers() {
    use ndex_core::config::Config;
    use ndex_store::Store;
    use ndex_store::lock::IndexLock;

    let tmp = tempfile::tempdir().unwrap();
    let mut created = Store::create(tmp.path(), test_identity(), Config::default()).unwrap();
    created
        .fts
        .add_chunk(1, &chunk(0, "hello world"), &fts_meta("a.txt", None))
        .unwrap();
    created.fts.commit().unwrap();
    drop(created);

    // Two read stores coexist (shared flock; writer-less FTS handles).
    let a = Store::open_read(tmp.path()).unwrap();
    let mut b = Store::open_read(tmp.path()).unwrap();
    assert_eq!(a.fts.search("hello", 10, 2.0).unwrap().len(), 1);
    assert_eq!(b.fts.search("hello", 10, 2.0).unwrap().len(), 1);
    assert!(a.manifest.live_files().unwrap().is_empty());

    // A read store cannot write to the FTS index...
    assert!(b.fts.commit().is_err());
    // ...and a writer cannot enter while readers hold the shared lock.
    let ndex_dir = tmp.path().join(ndex_core::constants::NDEX_DIR);
    assert!(IndexLock::try_acquire(&ndex_dir).unwrap().is_none());

    drop(a);
    drop(b);
    // Once the readers drop, an exclusive open succeeds again.
    let _writer = Store::open(tmp.path()).unwrap();
}
