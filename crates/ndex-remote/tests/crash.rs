//! SIGKILL crash-safety harness (PRD §18.1; specs/30-ingest/31-reconcile.md invariant).
//!
//! Kills `ndex-remote index` mid-run with SIGKILL, re-runs it to completion, then verifies the
//! crash-safety invariant directly against `.ndex/manifest.db`:
//!
//! (a) every sampled `status = Indexed` file's unique token is findable via `search`;
//! (b) a sampled token returns exactly ONE hit (no duplicate chunks from re-processing);
//! (c) indexed + skipped + failed row counts equal the corpus size.
//!
//! This is the durability regression gate — it must stay un-`#[ignore]`d.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rusqlite::Connection;

/// Initial corpus size (small unique-token text files).
const CORPUS_FILES: usize = 1000;
/// Files added per retry when a run finishes before the kill lands.
const RETRY_EXTRA_FILES: usize = 300;
/// Attempts to land a mid-run SIGKILL before giving up.
const MAX_KILL_ATTEMPTS: usize = 3;
/// How many indexed files to spot-check via the real `search` binary.
const SAMPLE_SIZE: usize = 25;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ndex-remote"))
}

/// Write `count` unique-token text files starting at ordinal `start`.
fn write_corpus(root: &Path, start: usize, count: usize) {
    for i in start..start + count {
        let token = format!("crashtok{i:05}");
        std::fs::write(
            root.join(format!("{token}.txt")),
            format!("{token} is the unique sentinel for this document in the crash corpus.\n"),
        )
        .unwrap();
    }
}

/// `SELECT COUNT(*) FROM files WHERE status = Indexed`, tolerating a busy/locked database
/// (returns `None` while the child writer holds it).
fn indexed_count(manifest: &Path) -> Option<i64> {
    let conn = Connection::open(manifest).ok()?;
    conn.query_row("SELECT COUNT(*) FROM files WHERE status = 1", [], |r| {
        r.get(0)
    })
    .ok()
}

fn spawn_index(root: &Path) -> Child {
    bin()
        .arg("index")
        .arg(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// Poll for evidence of progress (at least one batched commit beyond `baseline`), then
/// SIGKILL the child. Returns `true` if the kill landed mid-run, `false` if the child
/// finished first.
fn kill_mid_run(child: &mut Child, manifest: &Path, baseline: i64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if child.try_wait().unwrap().is_some() {
            return false; // finished before the kill landed
        }
        if indexed_count(manifest).unwrap_or(baseline) > baseline {
            child.kill().unwrap(); // std kill == SIGKILL on unix
            child.wait().unwrap();
            return true;
        }
        assert!(
            Instant::now() < deadline,
            "index child made no observable progress within 30s"
        );
        std::thread::sleep(Duration::from_millis(3));
    }
}

#[test]
fn sigkill_mid_index_then_rerun_preserves_crash_safety_invariant() {
    let dir = tempfile::tempdir().unwrap();
    let root: PathBuf = dir.path().to_path_buf();
    let manifest = root.join(".ndex/manifest.db");

    write_corpus(&root, 0, CORPUS_FILES);
    let mut corpus = CORPUS_FILES;

    assert!(
        bin()
            .arg("init")
            .arg(&root)
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success()
    );

    // Land a SIGKILL mid-run; if a run outraces the kill, grow the corpus and retry (bounded).
    let mut killed = false;
    for _attempt in 0..MAX_KILL_ATTEMPTS {
        let baseline = indexed_count(&manifest).unwrap_or(0);
        let mut child = spawn_index(&root);
        if kill_mid_run(&mut child, &manifest, baseline) {
            killed = true;
            break;
        }
        write_corpus(&root, corpus, RETRY_EXTRA_FILES);
        corpus += RETRY_EXTRA_FILES;
    }
    assert!(
        killed,
        "could not land a mid-run SIGKILL in {MAX_KILL_ATTEMPTS} attempts"
    );

    // Recovery run: must complete cleanly from whatever state the kill left behind.
    let rerun = bin().arg("index").arg(&root).output().unwrap();
    assert!(
        rerun.status.success(),
        "post-crash index run failed: {}",
        String::from_utf8_lossy(&rerun.stderr)
    );

    let conn = Connection::open(&manifest).unwrap();
    let count = |status: i64| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM files WHERE status = ?1",
            [status],
            |r| r.get(0),
        )
        .unwrap()
    };

    // (c) Full accounting: indexed(1) + failed(2,4) + skipped(5) == corpus; nothing left
    // pending(0) or spuriously deleted(3).
    let (indexed, skipped) = (count(1), count(5));
    let failed = count(2) + count(4);
    assert_eq!(
        indexed + skipped + failed,
        corpus as i64,
        "indexed={indexed} skipped={skipped} failed={failed} corpus={corpus}"
    );
    assert_eq!(count(0), 0, "no file may remain Pending after a clean run");
    assert_eq!(count(3), 0, "no file was deleted from the corpus");

    // (a)+(b) Sample random Indexed rows: each unique token must be findable and must hit
    // exactly one chunk (duplicates would mean a killed batch was re-added without delete).
    let mut stmt = conn
        .prepare("SELECT path FROM files WHERE status = 1 ORDER BY RANDOM() LIMIT ?1")
        .unwrap();
    let sampled: Vec<String> = stmt
        .query_map([SAMPLE_SIZE as i64], |r| r.get::<_, Vec<u8>>(0))
        .unwrap()
        .map(|bytes| String::from_utf8(bytes.unwrap()).unwrap())
        .collect();
    assert!(sampled.len() >= 20, "expected ≥20 indexed files to sample");

    for path in &sampled {
        let token = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap()
            .to_string();
        let out = bin()
            .arg("search")
            .arg(&root)
            .arg(&token)
            .args(["--format", "paths"])
            .output()
            .unwrap();
        assert!(out.status.success(), "search {token} failed");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let hits: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(
            hits.len(),
            1,
            "token {token} must return exactly one hit, got {}: {stdout}",
            hits.len()
        );
        assert!(
            hits[0].contains(&token),
            "hit for {token} should be its own file, got: {}",
            hits[0]
        );
    }
}
