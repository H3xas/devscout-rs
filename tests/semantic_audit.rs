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
//! `HOME`/`SCOUT_REGISTRY`/`SCOUT_CONTENT_DB` pointed at a fresh temp dir each --
//! same isolation rule and the same duplicated-per-file helpers as
//! `tests/cli_type_arity.rs` and `tests/cli_read.rs` (each `tests/*.rs` file
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
        self.run_env(args, &[])
    }

    fn run_env(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_devscout"));
        cmd.args(args)
            .current_dir(&self.repo)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"));
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.output().expect("devscout must run")
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

    assert_eq!(v["tiers"]["precise"]["tp"], 36, "{stdout}");
    assert_eq!(v["tiers"]["precise"]["fp"], 0, "{stdout}");
    assert_eq!(v["tiers"]["ext"]["tp"], 6, "{stdout}");
    assert_eq!(v["tiers"]["ext"]["fp"], 0, "{stdout}");
    assert_eq!(v["tiers"]["guess"]["tp"], 4, "{stdout}");
    assert_eq!(v["tiers"]["guess"]["fp"], 0, "{stdout}");
    assert!(
        v["tiers"]["heuristic"].is_null(),
        "the legacy umbrella tier is gone -- nothing untagged is left to fall into it: {stdout}"
    );
    let recall_all = v["recall"]["all"].as_f64().expect("recall.all is a number");
    // Every in-graph probe site in the fixture -- `this.`/`base.`/`?.`
    // receivers, awaited locals, casts, patterns, `out` designations,
    // cross-file field facts, one-hop chain tails, a single-parameter
    // lambda's element type, and arity-gated call vouching alike -- now
    // resolves through some tier, so the floor is 1.0 rather than a fraction
    // short of it: a miss anywhere in the fixture would be a regression, not
    // an accepted gap.
    assert!(recall_all >= 1.0, "recall.all = {recall_all}: {stdout}");
    let structural_impossible = v["structural"]["impossible"]
        .as_u64()
        .expect("structural.impossible is a number");
    assert!(
        structural_impossible == 0,
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

    // The same violated threshold with `--json`: stdout must stay ONE parseable
    // JSON object so a CI step can assert and pipe the report into `jq` in the
    // same run. The violation lines move to stderr instead of being appended
    // after the object, and the exit code is still 1.
    let failing_json = fx.run(&[
        "audit",
        "--semantic",
        refs.to_str().unwrap(),
        "--units",
        units.to_str().unwrap(),
        "--assert",
        violated.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(failing_json.status.code(), Some(1), "{failing_json:?}");
    let json_stdout = stdout_of(&failing_json);
    let json_stderr = String::from_utf8(failing_json.stderr.clone()).expect("stderr is utf-8");
    assert!(
        !json_stdout.contains("assert:"),
        "no violation line may reach stdout in --json mode: {json_stdout}"
    );
    assert!(
        json_stderr.contains("assert: tiers.precise.tp"),
        "the violation line goes to stderr instead: {json_stderr}"
    );
    let parsed: serde_json::Value = serde_json::from_str(json_stdout.trim())
        .expect("--json --assert stdout must parse as JSON");
    assert!(
        parsed.get("tiers").is_some(),
        "and it is the report object, not a fragment: {json_stdout}"
    );
}

/// Asking for the row file leaves the report itself alone: a caller that
/// reads stdout sees the same bytes whether or not the rows were written.
#[test]
fn writing_the_false_positive_rows_leaves_both_report_formats_byte_identical() {
    let fx = Fixture::build("fprows");
    let refs = refs_path();
    let units = units_path();
    let rows = fx.repo.join("rows.jsonl");
    for extra in [None, Some(rows.to_str().unwrap())] {
        for format in [vec![], vec!["--json"]] {
            let mut args = vec![
                "audit",
                "--semantic",
                refs.to_str().unwrap(),
                "--units",
                units.to_str().unwrap(),
            ];
            args.extend(format.iter().copied());
            if let Some(path) = extra {
                args.extend(["--fp-sites", path]);
            }
            let out = fx.run(&args);
            assert!(out.status.success(), "audit failed: {out:?}");
            let baseline = fx.repo.join(if format.is_empty() {
                "baseline.txt"
            } else {
                "baseline.json"
            });
            let stdout = stdout_of(&out);
            if extra.is_none() {
                fs::write(&baseline, &stdout).expect("record the baseline report");
            } else {
                let before = fs::read_to_string(&baseline).expect("read the baseline report");
                assert_eq!(before, stdout, "the row file changed the report");
            }
        }
    }
    assert!(rows.is_file(), "the row file must be written");
    let written = fs::read_to_string(&rows).expect("read the rows");
    assert!(
        written.is_empty(),
        "the fixture scores no false positive, so it has no rows: {written}"
    );
}

/// The emission record is an observation, never an input: a graph built with
/// it switched on is byte-for-byte the graph built without it, and the record
/// accounts for every `uses-member` edge that graph carries.
#[test]
fn recording_edge_provenance_does_not_change_the_graph_it_records() {
    let fx = Fixture::build("provenance");
    let graph = fx.repo.join(".scout/graph/graph.json");
    let unobserved = fs::read(&graph).expect("read the mapped graph");

    // A fresh graph short-circuits `map`, so the resolver would never run.
    fs::remove_file(&graph).expect("drop the graph so the remap resolves");
    let record = fx.repo.join("provenance.jsonl");
    let remap = fx.run_env(
        &["map", "src", "tests"],
        &[("SCOUT_EDGE_PROVENANCE", record.to_str().unwrap())],
    );
    assert!(remap.status.success(), "map failed: {remap:?}");
    assert_eq!(
        unobserved,
        fs::read(&graph).expect("read the observed graph"),
        "recording provenance changed the graph"
    );

    let rows = fs::read_to_string(&record).expect("read the provenance record");
    let parsed: Vec<serde_json::Value> = rows
        .lines()
        .map(|l| serde_json::from_str(l).expect("each row is valid JSON"))
        .collect();
    let graph_value: serde_json::Value =
        serde_json::from_slice(&unobserved).expect("the graph is valid JSON");
    let uses_member = graph_value["edges"]
        .as_array()
        .expect("edges is an array")
        .iter()
        .filter(|e| e["kind"] == "uses-member")
        .count();
    assert!(uses_member > 0, "the fixture must carry uses-member edges");
    assert_eq!(parsed.len(), uses_member, "one row per uses-member edge");
    assert!(
        parsed.iter().all(|r| r["step"].is_string()),
        "every edge must name the arm that emitted it: {rows}"
    );
}
