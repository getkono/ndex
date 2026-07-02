//! End-to-end pipeline tests (skeleton stubs; PRD §18).
//!
//! These run against `tests/fixtures/` once the pipeline is implemented. They are `#[ignore]`d so
//! the suite stays green; run them with `cargo test -- --ignored`.

#[test]
#[ignore = "skeleton: init → index tests/fixtures → assert manifest state + FTS/semantic hits (PRD §18.2)"]
fn init_index_search_roundtrip() {
    todo!()
}

#[test]
#[ignore = "skeleton: index all v0.1 formats; assert status/mime/blake3 + doc_meta/media_meta (PRD §18.2)"]
fn all_v0_1_formats_index_correctly() {
    todo!()
}

#[test]
#[ignore = "skeleton: SIGKILL mid-index at several points → restart → recover from status=0 (PRD §18.1)"]
fn crash_recovery_resumes_pending_files() {
    todo!()
}

#[test]
#[ignore = "skeleton: corrupt sidecar count → open detects mismatch → auto-repair (PRD §10.3, §18.1)"]
fn sidecar_usearch_mismatch_is_repaired() {
    todo!()
}

#[test]
#[ignore = "skeleton: thin client ↔ ssh localhost ↔ ndex-remote, incl. preamble + version negotiation (PRD §18.1)"]
fn ssh_transport_roundtrip() {
    todo!()
}
