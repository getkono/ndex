//! End-to-end pipeline tests against the `ndex-remote` standalone CLI (PRD §18).
//!
//! `init_index_search_roundtrip` runs the real local pipeline. The remaining cases (all v0.1
//! formats, crash recovery, sidecar repair, SSH transport) are `#[ignore]`d pending the vector
//! index, the serve loop, and the thin-client transport.

use assert_cmd::Command;
use predicates::prelude::*;

fn ndex_remote() -> Command {
    Command::cargo_bin("ndex-remote").unwrap()
}

#[test]
fn init_index_search_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("docs")).unwrap();
    std::fs::write(
        root.join("docs/report.md"),
        "# Q3\n\nQuarterly earnings grew sharply across every segment this year.\n",
    )
    .unwrap();
    std::fs::write(
        root.join("notes.txt"),
        "blake3 is a fast cryptographic hash function used for content addressing.\n",
    )
    .unwrap();

    // init creates the index.
    ndex_remote().arg("init").arg(root).assert().success();

    // index processes both files.
    ndex_remote()
        .arg("index")
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("2 new"))
        .stdout(predicate::str::contains("2 processed"))
        .stdout(predicate::str::contains("0 failed"));

    // FTS search finds the markdown doc by content.
    ndex_remote()
        .arg("search")
        .arg(root)
        .arg("earnings")
        .assert()
        .success()
        .stdout(predicate::str::contains("report.md"));

    // `--format paths` prints just the path.
    ndex_remote()
        .arg("search")
        .arg(root)
        .arg("blake3")
        .args(["--format", "paths"])
        .assert()
        .success()
        .stdout(predicate::str::contains("notes.txt"));

    // A query with no matches reports no results (and still exits 0).
    ndex_remote()
        .arg("search")
        .arg(root)
        .arg("zzzznomatch")
        .assert()
        .success();

    // Re-indexing is idempotent.
    ndex_remote()
        .arg("index")
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("2 unchanged"));

    // stats reports the file count.
    ndex_remote()
        .arg("stats")
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("files:"));
}

#[test]
#[ignore = "impl pending: all v0.1 formats (pdf/docx/html/image/archive) + doc_meta/media_meta"]
fn all_v0_1_formats_index_correctly() {
    todo!()
}

#[test]
#[ignore = "impl pending: SIGKILL mid-index → restart → recover from status=Pending (PRD §18.1)"]
fn crash_recovery_resumes_pending_files() {
    todo!()
}

#[test]
#[ignore = "impl pending: vector index — corrupt sidecar count → open detects → auto-repair"]
fn sidecar_usearch_mismatch_is_repaired() {
    todo!()
}

#[test]
#[ignore = "impl pending: thin client <-> ssh localhost <-> ndex-remote serve loop (PRD §18.1)"]
fn ssh_transport_roundtrip() {
    todo!()
}
