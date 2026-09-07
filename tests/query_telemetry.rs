//! Integration tests for `SCOUT_TELEMETRY`: the query log every `find`/
//! `refs`/`read`/`impact`/`tests` invocation appends a line to when the
//! switch is on.
//!
//! Every test here goes through the COMPILED BINARY as a subprocess, the same
//! convention `tests/cli_read.rs` documents: each `tests/*.rs` file compiles
//! as an independent binary, so its helpers below are deliberately
//! duplicated rather than shared.
//!
//! SAFETY: every spawned process gets HOME, SCOUT_REGISTRY and
//! SCOUT_CONTENT_DB pointed at a fresh temp dir, so the operator's real
//! `~/.claude/settings.json` and `repos.json` are never read or written.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-query-telemetry-{prefix}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn run_git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git binary must be on PATH");
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

fn bootstrap_initial_commit(dir: &Path) {
    const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
    let output = Command::new("git")
        .args(["commit-tree", EMPTY_TREE, "-m", "init"])
        .env("GIT_AUTHOR_NAME", "devscout-test")
        .env("GIT_AUTHOR_EMAIL", "devscout-test@example.com")
        .env("GIT_COMMITTER_NAME", "devscout-test")
        .env("GIT_COMMITTER_EMAIL", "devscout-test@example.com")
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .expect("git commit-tree must run");
    assert!(
        output.status.success(),
        "git commit-tree failed: {output:?}"
    );
    let sha = String::from_utf8(output.stdout).unwrap().trim().to_string();
    run_git(dir, &["update-ref", "refs/heads/master", &sha]);
    run_git(dir, &["symbolic-ref", "HEAD", "refs/heads/master"]);
}

fn rust_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_devscout"))
}

const FILES: &[(&str, &str)] = &[
    ("src/IThing.cs", "namespace Shop\n{\n    public interface IThing\n    {\n        int Id { get; }\n    }\n}\n"),
    ("src/Thing.cs", "namespace Shop\n{\n    public class Thing : IThing\n    {\n        public int Id { get; set; }\n    }\n}\n"),
    (
        "tests/ThingTests.cs",
        "using NUnit.Framework;\n\nnamespace Shop.Tests\n{\n    public class ThingTests\n    {\n        [Test]\n        public void Reads() { var t = new Thing(); }\n    }\n}\n",
    ),
];

struct Fixture {
    root: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn build(prefix: &str) -> Fixture {
        let base = temp_dir(prefix);
        let root = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&home).expect("create home dir");
        for (rel, body) in FILES {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).expect("create fixture dir");
            fs::write(&path, body).expect("write fixture file");
        }
        run_git(&root, &["init", "-q", "."]);
        bootstrap_initial_commit(&root);
        let fixture = Fixture { root, home };
        let init = fixture.run(&["init", "--no-hooks", "--no-map"], false);
        assert!(init.status.success(), "init failed: {init:?}");
        let map = fixture.run(&["map", "."], false);
        assert!(map.status.success(), "map failed: {map:?}");
        fixture
    }

    fn run(&self, args: &[&str], telemetry: bool) -> Output {
        let mut cmd = Command::new(rust_bin());
        cmd.args(args)
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"));
        if telemetry {
            cmd.env("SCOUT_TELEMETRY", "1");
        } else {
            cmd.env_remove("SCOUT_TELEMETRY");
        }
        cmd.output().expect("devscout must run")
    }

    fn log_dir(&self) -> PathBuf {
        self.root.join(".git").join("scout").join("log")
    }

    fn log_path(&self) -> PathBuf {
        self.log_dir().join("queries.jsonl")
    }

    fn log_lines(&self) -> Vec<String> {
        fs::read_to_string(self.log_path())
            .expect("queries.jsonl must be readable")
            .lines()
            .map(str::to_string)
            .collect()
    }
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

// A record's keys, in the fixed order the record's own contract promises, so
// this fails the moment a future edit reorders one.
const KEY_ORDER: &[&str] = &[
    "\"ts\":",
    "\"schema_version\":",
    "\"verb\":",
    "\"seed\":",
    "\"outcome\":",
    "\"elapsed_ms\":",
    "\"result_bytes\":",
    "\"candidate_count\":",
];

fn assert_key_order(line: &str) {
    let mut last = 0usize;
    for key in KEY_ORDER {
        let pos = line
            .find(key)
            .unwrap_or_else(|| panic!("{key} missing from {line}"));
        assert!(pos >= last, "{key} out of order in {line}");
        last = pos;
    }
}

#[test]
fn telemetry_off_by_default_creates_no_log_file_or_directory() {
    let fx = Fixture::build("off");
    for args in [
        vec!["find", "Thing"],
        vec!["refs", "IThing"],
        vec!["read", "IThing"],
        vec!["impact", "src/IThing.cs"],
        vec!["tests", "Thing"],
    ] {
        let out = fx.run(&args, false);
        assert!(out.status.success(), "{args:?} failed: {out:?}");
    }
    assert!(
        !fx.log_dir().exists(),
        "no scout/log directory without SCOUT_TELEMETRY=1"
    );
}

#[test]
fn any_value_other_than_the_literal_one_stays_off() {
    let fx = Fixture::build("off-other-value");
    let mut cmd = Command::new(rust_bin());
    cmd.args(["refs", "IThing"])
        .current_dir(&fx.root)
        .env("HOME", &fx.home)
        .env("SCOUT_REGISTRY", fx.home.join("repos.json"))
        .env("SCOUT_CONTENT_DB", fx.home.join("content.db"))
        .env("SCOUT_TELEMETRY", "true");
    let out = cmd.output().expect("devscout must run");
    assert!(out.status.success(), "{out:?}");
    assert!(!fx.log_dir().exists());
}

#[test]
fn refs_read_impact_tests_and_find_each_append_one_correctly_shaped_line() {
    let fx = Fixture::build("shapes");

    let refs_out = fx.run(&["refs", "IThing"], true);
    assert!(refs_out.status.success(), "{refs_out:?}");
    let read_out = fx.run(&["read", "IThing"], true);
    assert!(read_out.status.success(), "{read_out:?}");
    let impact_out = fx.run(&["impact", "src/IThing.cs"], true);
    assert!(impact_out.status.success(), "{impact_out:?}");
    let tests_out = fx.run(&["tests", "Thing"], true);
    assert!(tests_out.status.success(), "{tests_out:?}");
    let find_out = fx.run(&["find", "Thing"], true);
    assert!(find_out.status.success(), "{find_out:?}");

    let lines = fx.log_lines();
    assert_eq!(lines.len(), 5, "one line per invocation: {lines:#?}");

    for line in &lines {
        assert_key_order(line);
        assert!(line.contains("\"schema_version\":1"));
    }

    // `refs IThing`: Thing inherits IThing, one inbound row -- and the
    // record's own byte length matches the answer this same call printed,
    // modulo the one trailing newline `print_out` always adds.
    assert!(lines[0].contains("\"verb\":\"refs\""));
    assert!(lines[0].contains("\"seed\":\"IThing\""));
    assert!(lines[0].contains("\"outcome\":\"hit\""));
    assert!(lines[0].contains("\"candidate_count\":1"));
    let result_bytes: usize = lines[0]
        .rsplit("\"result_bytes\":")
        .next()
        .unwrap()
        .split(',')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        result_bytes + 1,
        stdout_of(&refs_out).len(),
        "result_bytes is the answer's own length, before print_out's trailing newline"
    );

    assert!(lines[1].contains("\"verb\":\"read\""));
    assert!(lines[1].contains("\"seed\":\"IThing\""));
    assert!(lines[1].contains("\"outcome\":\"hit\""));
    assert!(lines[1].contains("\"candidate_count\":1"));

    assert!(lines[2].contains("\"verb\":\"impact\""));
    assert!(lines[2].contains("\"seed\":\"src/IThing.cs\""));
    assert!(lines[2].contains("\"outcome\":\"hit\""));

    assert!(lines[3].contains("\"verb\":\"tests\""));
    assert!(lines[3].contains("\"seed\":\"Thing\""));
    assert!(lines[3].contains("\"outcome\":\"hit\""));
    assert!(lines[3].contains("\"candidate_count\":1"));

    assert!(lines[4].contains("\"verb\":\"find\""));
    assert!(lines[4].contains("\"seed\":\"Thing\""));
    assert!(lines[4].contains("\"outcome\":\"hit\""));
}

// `Reads` is declared on `ThingTests` and referenced nowhere, so `refs`
// resolves it to an empty answer: the outcome recorded has to be the one the
// caller was handed, not the unresolved word an exit code alone would suggest.
#[test]
fn a_resolved_member_with_nothing_referencing_it_records_zero_hit() {
    let fx = Fixture::build("member-zero-hit");
    let out = fx.run(&["refs", "Reads"], true);
    assert_eq!(out.status.code(), Some(3), "{out:?}");

    let lines = fx.log_lines();
    assert_eq!(lines.len(), 1, "{lines:#?}");
    assert_key_order(&lines[0]);
    assert!(lines[0].contains("\"verb\":\"refs\""), "{}", lines[0]);
    assert!(lines[0].contains("\"seed\":\"Reads\""), "{}", lines[0]);
    assert!(
        lines[0].contains("\"outcome\":\"zero-hit\""),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains("\"candidate_count\":0"), "{}", lines[0]);
}

#[test]
fn a_second_invocation_appends_rather_than_truncates() {
    let fx = Fixture::build("append");
    let first = fx.run(&["refs", "IThing"], true);
    assert!(first.status.success(), "{first:?}");
    let second = fx.run(&["refs", "Thing"], true);
    assert!(second.status.success(), "{second:?}");

    let lines = fx.log_lines();
    assert_eq!(lines.len(), 2, "the second call must append, not truncate");
    assert!(lines[0].contains("\"seed\":\"IThing\""));
    assert!(lines[1].contains("\"seed\":\"Thing\""));
}

#[test]
fn an_unwritable_log_path_leaves_exit_code_and_output_unchanged() {
    let fx = Fixture::build("unwritable");
    let baseline = fx.run(&["refs", "IThing"], false);
    assert!(baseline.status.success(), "{baseline:?}");

    // The log FILE'S OWN path exists as a directory ahead of time, so opening
    // it for append fails; this must not be visible in the exit code or
    // stdout at all.
    fs::create_dir_all(fx.log_path()).expect("pre-create log path as a directory");
    let broken = fx.run(&["refs", "IThing"], true);

    assert_eq!(baseline.status.code(), broken.status.code());
    assert_eq!(stdout_of(&baseline), stdout_of(&broken));
    assert!(
        fx.log_path().is_dir(),
        "an unwritable log is left exactly as broken as it was found, never touched"
    );
}
