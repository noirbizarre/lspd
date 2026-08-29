//! End-to-end CLI tests, driving the real compiled binary via `assert_cmd`.
//!
//! Unlike `tests/daemon.rs` (which runs the daemon in-process), commands
//! here that need a daemon go through the actual `discover_or_start`
//! subprocess-spawn path: `std::env::current_exe()` inside the spawned
//! process resolves to this same `projectd` binary, so this is the one place
//! that exercises the real "cargo test binary calls a real projectd binary
//! which execs another real projectd binary" chain end to end.

use std::fs;
use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn projectd() -> Command {
    Command::cargo_bin("projectd").expect("the projectd binary should build")
}

/// Tests that go through the real subprocess discover-or-start path leave a
/// real detached daemon process running — nothing in the CLI itself ever
/// tears one down, since a daemon's whole point is to outlive any single
/// invocation. Tests must therefore clean up after themselves explicitly, or
/// repeated local `cargo test` runs accumulate zombie daemons pointing at
/// already-deleted temp directories.
fn shutdown_daemon(project_root: &std::path::Path) {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    let Ok(root) = projectd::project::ProjectRoot::discover(project_root) else {
        return;
    };
    let socket_path = projectd::daemon::socket::control_socket_path(root.path());
    let Ok(mut stream) = UnixStream::connect(&socket_path) else {
        return;
    };

    let mut buf = [0u8; 4096];
    let _ = stream.write_all(b"{\"type\":\"handshake\"}\n");
    let _ = stream.read(&mut buf);
    let _ = stream.write_all(b"{\"type\":\"shutdown\"}\n");
    let _ = stream.read(&mut buf);
}

fn python_project() -> TempDir {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("pyproject.toml"), "").unwrap();
    tmp
}

#[test]
fn help_lists_every_top_level_command_group() {
    projectd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("project"))
        .stdout(predicate::str::contains("lsp"))
        .stdout(predicate::str::contains("events"))
        .stdout(predicate::str::contains("watch"))
        .stdout(predicate::str::contains("task"))
        .stdout(predicate::str::contains("mcp"));
}

#[test]
fn project_info_on_a_zero_config_python_project_shows_both_registry_lsps() {
    let tmp = python_project();
    projectd()
        .args(["--project", tmp.path().to_str().unwrap(), "project", "info"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pyright"))
        .stdout(predicate::str::contains("ruff"));
}

#[test]
fn project_info_json_envelope_matches_the_documented_shape() {
    let tmp = python_project();
    let output = projectd()
        .args([
            "--project",
            tmp.path().to_str().unwrap(),
            "--json",
            "project",
            "info",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value =
        serde_json::from_slice(&output).expect("stdout must be valid JSON");
    assert_eq!(value["version"], 1);
    assert_eq!(value["status"], "ok");
    let enabled = value["payload"]["enabled_lsps"].as_array().unwrap();
    let names: Vec<&str> = enabled.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(names.contains(&"pyright"));
    assert!(names.contains(&"ruff"));
}

#[test]
fn lsp_config_reports_pyright_and_ruff_as_independently_configured() {
    let tmp = python_project();

    projectd()
        .args([
            "--project",
            tmp.path().to_str().unwrap(),
            "lsp",
            "config",
            "pyright",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("pyright-langserver"));

    projectd()
        .args([
            "--project",
            tmp.path().to_str().unwrap(),
            "lsp",
            "config",
            "ruff",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ruff"));
}

#[test]
fn lsp_config_for_an_unknown_name_fails_with_a_clear_diagnostic() {
    let tmp = python_project();
    projectd()
        .args([
            "--project",
            tmp.path().to_str().unwrap(),
            "lsp",
            "config",
            "does-not-exist",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does-not-exist"));
}

#[test]
fn a_project_toml_can_override_one_definition_without_changing_activation() {
    let tmp = python_project();
    fs::write(
        tmp.path().join("projectd.toml"),
        r#"
        [lsp.pyright]
        command = ["basedpyright-langserver", "--stdio"]
        "#,
    )
    .unwrap();

    projectd()
        .args([
            "--project",
            tmp.path().to_str().unwrap(),
            "lsp",
            "config",
            "pyright",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("basedpyright-langserver"));

    projectd()
        .args([
            "--project",
            tmp.path().to_str().unwrap(),
            "--json",
            "project",
            "info",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""pyright""#))
        .stdout(predicate::str::contains(r#""ruff""#));
}

/// End-to-end validation of spec §14's discover-or-start lifecycle and §15's
/// instance reuse, through the real subprocess-spawning `client` module —
/// not the in-process daemon harness `tests/daemon.rs` uses.
#[test]
fn lsp_ensure_starts_a_real_detached_daemon_that_status_and_endpoint_then_reuse() {
    let tmp = python_project();
    fs::write(
        tmp.path().join("projectd.toml"),
        r#"
        [lsps]
        enabled = ["stub"]

        [lsp.stub]
        command = ["cat"]
        "#,
    )
    .unwrap();
    let project = tmp.path().to_str().unwrap();

    projectd()
        .args(["--project", project, "lsp", "ensure", "stub"])
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .stdout(predicate::str::contains("Running").or(predicate::str::contains("running")));

    projectd()
        .args(["--project", project, "lsp", "status", "stub"])
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let endpoint_output = projectd()
        .args(["--project", project, "lsp", "endpoint", "stub"])
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let socket_path = String::from_utf8(endpoint_output)
        .unwrap()
        .trim()
        .to_string();
    assert!(
        std::path::Path::new(&socket_path).exists(),
        "the reported proxy socket path should exist on disk: {socket_path}"
    );

    // `status` (which never starts a daemon on its own) now sees one running.
    projectd()
        .args(["--project", project, "status"])
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .stdout(predicate::str::contains("running"));

    shutdown_daemon(tmp.path());
}
