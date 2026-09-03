//! Integration tests for `devscout audit --semantic`.
//!
//! Reproduces, under `cargo test` and with no `dotnet` toolchain on PATH, the
//! exact flow CI's `semantic-audit` job (`.github/workflows/ci.yml`) runs
//! against the committed oracle snapshot: copy `fixtures/csharp-semantic`'s
//! `src`/`tests` into an isolated repo dir, `init --no-hooks --no-map`, `map
//! src tests`, then `audit --semantic <oracle/refs.jsonl> --units
//! <oracle/units.jsonl>`. The oracle itself (`tools/scout-semantic`) is never
//! invoked here -- `oracle/refs.jsonl` and `oracle/units.jsonl` are
//! committed, Roslyn-derived ground truth, and this file only checks that
//! `devscout`'s own graph still scores against them the way the fixture's
//! `expected.json` thresholds (and CI) expect.
//!
//! Every test here goes through the COMPILED BINARY as a subprocess, with
//! HOME/SCOUT_REGISTRY/SCOUT_CONTENT_DB pointed at a fresh temp dir each --
//! same isolation rule and the same duplicated-per-file helpers as
//! tests/cli_type_arity.rs and tests/cli_read.rs (each tests/*.rs file
//! compiles as an independent binary, so sharing these small helpers via a
//! common module would buy little).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-semantic-audit-{prefix}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-semantic")
}

/// Recursive copy of `src` into `dst`, skipping any directory named `bin` or
/// `obj` -- the same exclusion CI's `rsync -a --exclude bin --exclude obj`
/// applies when it stages an isolated copy of the fixture for indexing.
fn copy_tree_skip_build_output(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create dest dir");
    for entry in fs::read_dir(src).expect("read source dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name();
        let file_type = entry.file_type().expect("entry file type");
        if file_type.is_dir() {
            if name == "bin" || name == "obj" {
                continue;
            }
            copy_tree_skip_build_output(&entry.path(), &dst.join(&name));
        } else if file_type.is_file() {
            fs::copy(entry.path(), dst.join(&name)).expect("copy fixture file");
        }
    }
}

struct Fixture {
    repo: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn build(prefix: &str) -> Fixture {
        let base = temp_dir(prefix);
        let repo = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&repo).expect("create repo dir");
        fs::create_dir_all(&home).expect("create home dir");

        let src_root = fixture_root();
        copy_tree_skip_build_output(&src_root.join("src"), &repo.join("src"));
        copy_tree_skip_build_output(&src_root.join("tests"), &repo.join("tests"));

        let fx = Fixture { repo, home };
        let init = fx.run(&["init", "--no-hooks", "--no-map"]);
        assert!(init.status.success(), "init failed: {init:?}");
        let map = fx.run(&["map", "src", "tests"]);
        assert!(map.status.success(), "map failed: {map:?}");
        fx
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .args(args)
            .current_dir(&self.repo)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .output()
            .expect("devscout must run")
    }
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

fn refs_path() -> PathBuf {
    fixture_root().join("oracle/refs.jsonl")
}

fn units_path() -> PathBuf {
    fixture_root().join("oracle/units.jsonl")
}

fn expected_path() -> PathBuf {
    fixture_root().join("expected.json")
}

#[test]
fn audit_scores_the_fixture_against_the_committed_oracle_snapshot() {
    let fx = Fixture::build("score");
    let refs = refs_path();
    let units = units_path();
    let out = fx.run(&[
        "audit",
        "--semantic",
        refs.to_str().unwrap(),
        "--units",
        units.to_str().unwrap(),
        "--json",
    ]);
    assert!(out.status.success(), "audit --json failed: {out:?}");
    let stdout = stdout_of(&out);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    assert_eq!(v["tiers"]["precise"]["tp"], 7, "{stdout}");
    assert_eq!(v["tiers"]["precise"]["fp"], 0, "{stdout}");
    assert!(
        v["tiers"]["ext"].is_object(),
        "the extension tier is reported on its own now that every guess edge names its tier: {stdout}"
    );
    assert!(
        v["tiers"]["guess"].is_object(),
        "and so is the scored tier: {stdout}"
    );
    assert!(
        v["tiers"]["heuristic"].is_null(),
        "the legacy umbrella tier is gone -- nothing untagged is left to fall into it: {stdout}"
    );
    let recall_all = v["recall"]["all"].as_f64().expect("recall.all is a number");
    assert!(recall_all >= 0.9, "recall.all = {recall_all}: {stdout}");
    let structural_impossible = v["structural"]["impossible"]
        .as_u64()
        .expect("structural.impossible is a number");
    assert!(
        structural_impossible <= 2,
        "structural.impossible = {structural_impossible}: {stdout}"
    );
    assert_eq!(v["units"]["failed"], 0, "{stdout}");
}

#[test]
fn audit_assert_passes_the_fixture_thresholds_and_fails_a_violated_one() {
    let fx = Fixture::build("assert");
    let refs = refs_path();
    let units = units_path();
    let expected = expected_path();

    let passing = fx.run(&[
        "audit",
        "--semantic",
        refs.to_str().unwrap(),
        "--units",
        units.to_str().unwrap(),
        "--assert",
        expected.to_str().unwrap(),
    ]);
    assert_eq!(
        passing.status.code(),
        Some(0),
        "the fixture must pass its own thresholds today: {passing:?}"
    );
    let passing_stdout = stdout_of(&passing);
    assert!(
        passing_stdout.contains("devscout audit --semantic"),
        "the report is printed on stdout even with --assert: {passing_stdout}"
    );

    // A deliberately-violated threshold: the fixture always has more than
    // zero precise-tier true positives, so this must fail.
    let violated = fx.repo.join("violated-thresholds.json");
    fs::write(&violated, r#"{"tiers.precise.tp": {"max": 0}}"#).expect("write thresholds file");
    let failing = fx.run(&[
        "audit",
        "--semantic",
        refs.to_str().unwrap(),
        "--units",
        units.to_str().unwrap(),
        "--assert",
        violated.to_str().unwrap(),
    ]);
    assert_eq!(failing.status.code(), Some(1), "{failing:?}");
    let failing_stdout = stdout_of(&failing);
    assert!(
        failing_stdout.contains("assert:"),
        "a violated threshold appends an `assert:` line: {failing_stdout}"
    );
}
