//! Server standalone-CLI integration tests (real).

use assert_cmd::Command;
use predicates::str::contains;

fn remote() -> Command {
    Command::cargo_bin("ndex-remote").unwrap()
}

#[test]
fn prints_version() {
    remote()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains("ndex-remote"));
}

#[test]
fn help_lists_serve_and_model() {
    remote()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("serve"))
        .stdout(contains("model"));
}

#[test]
fn v0_2_commands_are_unavailable_with_exit_1() {
    for cmd in ["tag", "dedup", "compact"] {
        remote()
            .arg(cmd)
            .assert()
            .failure()
            .code(1)
            .stderr(contains("planned for v0.2"));
    }
}

#[test]
fn self_update_prints_v0_2_notice() {
    remote()
        .arg("self-update")
        .assert()
        .success()
        .stdout(contains("planned for v0.2"));
}

#[test]
fn generates_shell_completions() {
    remote().args(["completions", "zsh"]).assert().success();
}

// ---------------------------------------------------------------------------
// Error-path exit codes, observed from the binary (specs/10-core/14-errors.md).
// ---------------------------------------------------------------------------

#[test]
fn search_and_stats_without_index_exit_3() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    remote()
        .arg("search")
        .arg(root)
        .arg("anything")
        .assert()
        .failure()
        .code(3)
        .stderr(contains("index not found"));

    remote()
        .arg("stats")
        .arg(root)
        .assert()
        .failure()
        .code(3)
        .stderr(contains("index not found"));
}

#[test]
fn init_on_initialized_root_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    remote().arg("init").arg(root).assert().success();

    // `Store::create` maps "already initialized" to `NdexError::Other` (general error).
    remote()
        .arg("init")
        .arg(root)
        .assert()
        .failure()
        .code(1)
        .stderr(contains("an index already exists"));
}

#[test]
fn index_with_unparsable_max_file_size_is_a_config_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    remote().arg("init").arg(root).assert().success();

    remote()
        .arg("index")
        .arg(root)
        .args(["--max-file-size", "garbage"])
        .assert()
        .failure()
        .code(78)
        .stderr(contains("invalid --max-file-size"));
}

#[test]
fn init_warns_on_unimplemented_flags() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    remote()
        .arg("init")
        .arg(root)
        .args(["--exclude", "*.log", "--no-fts", "--no-meta"])
        .assert()
        .success()
        .stderr(contains("--exclude is not implemented in v0.1; ignoring"))
        .stderr(contains("--no-fts is not implemented in v0.1; ignoring"))
        .stderr(contains("--no-meta is not implemented in v0.1; ignoring"));
}

#[test]
fn index_fails_fast_when_lock_is_held() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    remote().arg("init").arg(root).assert().success();

    // Hold the exclusive flock from this process; the child must not block on it.
    let ndex_dir = root.join(".ndex");
    let _lock = ndex_store::IndexLock::acquire(&ndex_dir).unwrap();

    remote()
        .arg("index")
        .arg(root)
        .timeout(std::time::Duration::from_secs(20))
        .assert()
        .failure()
        .code(1)
        .stderr(contains("another ndex process holds the index lock"));
}
