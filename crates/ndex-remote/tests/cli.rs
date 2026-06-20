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
