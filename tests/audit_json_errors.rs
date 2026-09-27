//! Integration tests for where `audit` sends a fatal error. With `--json`
//! anywhere in its arguments, stdout is reserved for the one JSON report, so
//! every error goes to stderr with exit 1 and stdout stays empty: a CI step
//! that redirects stdout into a file still shows why it failed. Text mode,
//! `--assert` violations and the global `-C` failure keep their streams.
//!
//! See `fixtures/csharp-enrichment/README.md` for the mapped tree.

mod support;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use devscout_rs::graph;
use support::Fixture;

fn run_in(fx: &Fixture, dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_devscout"))
        .args(args)
        .current_dir(dir)
        .env("HOME", &fx.home)
        .env("SCOUT_REGISTRY", fx.home.join("repos.json"))
        .env("SCOUT_CONTENT_DB", fx.home.join("content.db"))
        .output()
        .expect("devscout must run")
}

/// A mapped copy of the fixture and an empty oracle beside it.
fn mapped(prefix: &str) -> (Fixture, String) {
    let fx = Fixture::build(prefix);
    fx.map(&[]);
    let refs = fx.home.join("refs.jsonl");
    fs::write(&refs, "").unwrap();
    (fx, refs.to_str().unwrap().to_string())
}

fn home_file(fx: &Fixture, name: &str, body: &str) -> String {
    let path = fx.home.join(name);
    fs::write(&path, body).unwrap();
    path.to_str().unwrap().to_string()
}

fn missing(fx: &Fixture, name: &str) -> String {
    fx.home.join(name).to_str().unwrap().to_string()
}

/// Exit 1, nothing at all on stdout, and one stderr line starting `error: `
/// that contains `needle`.
fn assert_error_on_stderr(out: &Output, needle: &str) {
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty(), "stdout must stay empty: {out:?}");
    let stderr = String::from_utf8(out.stderr.clone()).unwrap();
    assert!(stderr.starts_with("error: "), "{stderr}");
    assert!(stderr.contains(needle), "expected {needle:?} in {stderr:?}");
}

#[test]
fn an_argument_error_goes_to_stderr_wherever_json_appears() {
    let (fx, refs) = mapped("json-args");
    for args in [
        vec!["audit", "--json", "--bogus"],
        vec!["audit", "--bogus", "--json"],
        vec!["audit", "--semantic", &refs, "--bogus", "--json"],
    ] {
        assert_error_on_stderr(&fx.run(&args), "unrecognized argument '--bogus'");
    }
}

#[test]
fn a_missing_flag_value_goes_to_stderr() {
    let (fx, refs) = mapped("json-missing-value");
    assert_error_on_stderr(
        &fx.run(&["audit", "--json", "--semantic", &refs, "--units"]),
        "missing value for '--units'",
    );
}

#[test]
fn a_missing_semantic_flag_goes_to_stderr() {
    let (fx, _) = mapped("json-no-semantic");
    assert_error_on_stderr(
        &fx.run(&["audit", "--json"]),
        "--semantic <refs.jsonl> is required",
    );
}

#[test]
fn json_as_the_value_of_semantic_still_routes_the_error_to_stderr() {
    let (fx, _) = mapped("json-as-value");
    let out = fx.run(&["audit", "--semantic", "--json"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
    assert!(!out.stderr.is_empty(), "{out:?}");
}

#[test]
fn no_repository_root_goes_to_stderr() {
    let (fx, refs) = mapped("json-no-root");
    let outside = fx.home.join("not-a-repo");
    fs::create_dir_all(&outside).unwrap();
    assert_error_on_stderr(
        &run_in(&fx, &outside, &["audit", "--semantic", &refs, "--json"]),
        "",
    );
}

#[test]
fn no_graph_goes_to_stderr() {
    let fx = Fixture::build("json-no-graph");
    let refs = home_file(&fx, "refs.jsonl", "");
    assert_error_on_stderr(
        &fx.run(&["audit", "--semantic", &refs, "--json"]),
        "no graph.json for this repo",
    );
}

#[test]
fn a_graph_that_is_not_json_goes_to_stderr() {
    let (fx, refs) = mapped("json-bad-graph");
    fs::write(graph::graph_json_path(&fx.repo), "not json").unwrap();
    assert_error_on_stderr(
        &fx.run(&["audit", "--semantic", &refs, "--json"]),
        "is not valid JSON",
    );
}

#[test]
fn unreadable_or_malformed_oracle_files_go_to_stderr() {
    let (fx, refs) = mapped("json-bad-inputs");
    let absent = missing(&fx, "absent-refs.jsonl");
    assert_error_on_stderr(&fx.run(&["audit", "--semantic", &absent, "--json"]), "");

    let malformed = home_file(&fx, "malformed.jsonl", "{\"file\": 3}\n");
    assert_error_on_stderr(
        &fx.run(&["audit", "--semantic", &malformed, "--json"]),
        "malformed.jsonl",
    );

    let absent_units = missing(&fx, "absent-units.jsonl");
    assert_error_on_stderr(
        &fx.run(&[
            "audit",
            "--semantic",
            &refs,
            "--units",
            &absent_units,
            "--json",
        ]),
        "",
    );
}

#[test]
fn an_fp_sites_file_that_cannot_be_written_goes_to_stderr() {
    let (fx, refs) = mapped("json-fp-sites");
    let unwritable = missing(&fx, "no-such-dir/fp.jsonl");
    assert_error_on_stderr(
        &fx.run(&[
            "audit",
            "--semantic",
            &refs,
            "--fp-sites",
            &unwritable,
            "--json",
        ]),
        "failed to write",
    );
}

#[test]
fn an_assert_file_that_cannot_be_read_or_parsed_goes_to_stderr() {
    let (fx, refs) = mapped("json-bad-assert");
    let cases = [
        (missing(&fx, "absent-assert.json"), "failed to read"),
        (
            home_file(&fx, "not-json.json", "not json"),
            "assert file is not valid JSON",
        ),
        (
            home_file(&fx, "no-bound.json", r#"{"recall.all": {}}"#),
            "must carry a numeric",
        ),
    ];
    for (assert_path, needle) in cases {
        assert_error_on_stderr(
            &fx.run(&[
                "audit",
                "--semantic",
                &refs,
                "--json",
                "--assert",
                &assert_path,
            ]),
            needle,
        );
    }
}

#[test]
fn an_assert_violation_keeps_the_report_on_stdout() {
    let (fx, refs) = mapped("json-violation");
    let pin = home_file(&fx, "pin.json", r#"{"oracle.records": {"min": 1}}"#);
    let out = fx.run(&["audit", "--semantic", &refs, "--json", "--assert", &pin]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["status"], "ok");
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "assert: oracle.records = 0 < min 1\n"
    );
}

#[test]
fn text_mode_keeps_the_error_on_stdout() {
    let (fx, _) = mapped("text-error");
    let out = fx.run(&["audit", "--bogus"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stderr.is_empty(), "{out:?}");
    assert!(String::from_utf8(out.stdout)
        .unwrap()
        .starts_with("error: unrecognized argument '--bogus'"));
}

#[test]
fn the_global_directory_failure_is_unchanged_under_json() {
    let (fx, refs) = mapped("json-global-c");
    let absent = missing(&fx, "no-such-dir");
    let out = fx.run(&["-C", &absent, "audit", "--semantic", &refs, "--json"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8(out.stdout)
        .unwrap()
        .starts_with("error: "));
}
