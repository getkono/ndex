//! Thin-client CLI integration tests (real).

use assert_cmd::Command;
use predicates::str::contains;

fn ndex() -> Command {
    Command::cargo_bin("ndex").unwrap()
}

#[test]
fn prints_version() {
    ndex()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains("ndex"));
}

#[test]
fn help_lists_core_commands() {
    ndex()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("search"))
        .stdout(contains("index"))
        .stdout(contains("completions"));
}

#[test]
fn v0_2_commands_are_unavailable_with_exit_1() {
    for cmd in ["tag", "dedup", "compact"] {
        ndex()
            .arg(cmd)
            .assert()
            .failure()
            .code(1)
            .stderr(contains("planned for v0.2"));
    }
}

#[test]
fn generates_shell_completions() {
    ndex().args(["completions", "bash"]).assert().success();
}

#[test]
fn unknown_command_is_a_usage_error() {
    // clap reports usage errors with exit code 2.
    ndex()
        .arg("definitely-not-a-command")
        .assert()
        .failure()
        .code(2);
}
