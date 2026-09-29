//! End-to-end CLI tests against a temporary todo.txt.

use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;

/// Create a throwaway todo directory with a bash config and one task.
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let dot = dir.path().join(".todo");
    fs::create_dir_all(&dot).unwrap();
    let config = dot.join("config");
    fs::write(
        &config,
        format!(
            "export TODO_DIR=\"{dir}\"\nexport TODO_FILE=\"$TODO_DIR/todo.txt\"\nexport DONE_FILE=\"$TODO_DIR/done.txt\"\n",
            dir = dir.path().display()
        ),
    )
    .unwrap();
    fs::write(
        dir.path().join("todo.txt"),
        "(A) 2026-09-01 first +core\nsecond +core\n",
    )
    .unwrap();
    (dir, config)
}

fn todo(config: &PathBuf) -> Command {
    let mut cmd = Command::cargo_bin("todo").unwrap();
    cmd.arg("-d").arg(config);
    cmd
}

#[test]
fn list_shows_tasks() {
    let (_dir, config) = fixture();
    todo(&config)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("first +core"))
        .stdout(predicate::str::contains("second +core"));
}

#[test]
fn add_creates_a_task() {
    let (dir, config) = fixture();
    todo(&config).args(["add", "third +app"]).assert().success();
    let text = fs::read_to_string(dir.path().join("todo.txt")).unwrap();
    assert!(text.contains("third +app"));
}

#[test]
fn do_archives_to_done() {
    let (dir, config) = fixture();
    todo(&config).args(["do", "1"]).assert().success();
    let todo_text = fs::read_to_string(dir.path().join("todo.txt")).unwrap();
    assert!(!todo_text.contains("first +core"));
    let done = fs::read_to_string(dir.path().join("done.txt")).unwrap();
    assert!(done.contains("first +core"));
}

#[test]
fn json_output_is_valid() {
    let (_dir, config) = fixture();
    let out = todo(&config)
        .args(["--json", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(value.as_array().unwrap().len() == 2);
    assert_eq!(value[0]["priority"], "A");
}

#[test]
fn regex_filter_is_applied() {
    let (_dir, config) = fixture();
    todo(&config)
        .args(["-p", "--regex", "list", "^second"])
        .assert()
        .success()
        .stdout(predicate::str::contains("second"))
        .stdout(predicate::str::contains("first").not());
}

#[test]
fn report_summarises() {
    let (_dir, config) = fixture();
    todo(&config)
        .arg("report")
        .assert()
        .success()
        .stdout(predicate::str::contains("2 total, 2 open, 0 done"));
}
