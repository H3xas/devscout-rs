//! Integration tests for the comment-hygiene hook script.
//!
//! Each fixture under tests/data/comment_hygiene/ holds one Claude Code hook
//! payload, plain JSON. This file feeds each one to the script on stdin and
//! checks the exit code and, where relevant, the stderr reason line. The
//! fixture directory is excluded from `--scan` itself, so its JSON content
//! can stay realistic without tripping the checks it demonstrates.

use std::path::PathBuf;
use std::process::{Command, Output};

fn script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".claude/hooks/comment-hygiene.py")
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/comment_hygiene")
        .join(name)
}

fn run_with_fixture(name: &str) -> Output {
    let payload = std::fs::read(fixture_path(name)).expect("read fixture");
    let mut child = Command::new("python3")
        .arg(script_path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn comment-hygiene.py");

    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("child stdin")
            .write_all(&payload)
            .expect("write payload");
    }

    child.wait_with_output().expect("wait for child")
}

#[test]
fn edit_with_narrative_case_note_is_denied() {
    let out = run_with_fixture("edit_case_note.json");
    assert_eq!(out.status.code(), Some(2), "expected exit 2, got {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("comment-hygiene:"),
        "stderr missing reason prefix: {stderr}"
    );
}

#[test]
fn edit_with_ordinary_comment_is_allowed() {
    let out = run_with_fixture("edit_clean_utf8.json");
    assert_eq!(out.status.code(), Some(0), "expected exit 0, got {out:?}");
}

#[test]
fn git_commit_with_attribution_trailer_is_denied() {
    let out = run_with_fixture("bash_commit_trailer.json");
    assert_eq!(out.status.code(), Some(2), "expected exit 2, got {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("comment-hygiene:"),
        "stderr missing reason prefix: {stderr}"
    );
}

#[test]
fn write_with_clean_rust_is_allowed() {
    let out = run_with_fixture("write_clean.json");
    assert_eq!(out.status.code(), Some(0), "expected exit 0, got {out:?}");
}

#[test]
fn edit_naming_the_host_product_claude_code_is_allowed() {
    let out = run_with_fixture("edit_claude_code_allowed.json");
    assert_eq!(out.status.code(), Some(0), "expected exit 0, got {out:?}");
}

#[test]
fn edit_with_plan_label_is_denied() {
    let out = run_with_fixture("edit_plan_label.json");
    assert_eq!(out.status.code(), Some(2), "expected exit 2, got {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("plan label"),
        "stderr missing plan-label class: {stderr}"
    );
}

#[test]
fn edit_naming_a_resolver_step_is_allowed() {
    let out = run_with_fixture("edit_resolver_step_allowed.json");
    assert_eq!(out.status.code(), Some(0), "expected exit 0, got {out:?}");
}
