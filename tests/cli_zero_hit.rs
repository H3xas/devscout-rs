// Zero-hit exits: a query that ran and found nothing leaves on its own code,
// distinct from the environment (1) and usage (2) codes, and says so on stderr
// in one fixed line per verb.
//
// Every test here goes through the COMPILED BINARY as a subprocess, because
// the stream the line lands on is half of what is under test -- stdout has to
// stay byte-identical to what a zero hit printed before the stderr note
// existed, so an in-process call on `cmd_*` could not tell the two apart.
//
// SAFETY: every spawned process gets HOME, SCOUT_REGISTRY and SCOUT_CONTENT_DB
// pointed at a fresh temp dir, so the operator's real ~/.claude/settings.json
// and repos.json are never read and never written.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

/// The four fixed lines, spelled out here rather than imported from
/// `src/cli.rs`: an integration test asserts what a caller actually reads off
/// stderr, so a silent edit to the wording upstream has to fail here.
const ZERO_HIT_FIND: &str = "devscout find: zero hits — the manifest was searched and nothing matched. Not an error; fall back to text search (rg/grep) rather than rephrasing.";
const ZERO_HIT_REFS: &str = "devscout refs: zero hits — the graph was searched and no symbol matched. Not an error; fall back to text search (rg/grep) rather than rephrasing.";
const ZERO_HIT_IMPACT: &str = "devscout impact: zero hits — the graph was searched and no affected file came back. Not an error; fall back to text search (rg/grep) rather than rephrasing.";
const ZERO_HIT_TESTS: &str = "devscout tests: zero hits — the graph was searched and no symbol matched. Not an error; fall back to text search (rg/grep) rather than rephrasing.";

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-zero-hit-{prefix}-{}-{n}",
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

/// An initial commit without ever invoking `git commit` -- repository policy,
/// even for a throwaway temp-dir fixture.
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

/// `Island` is the shape the zero-edge case needs: a type nothing references
/// and that references nothing, so its blast radius is empty while the seed
/// itself resolves.
const FILES: &[(&str, &str)] = &[
    ("src/IThing.cs", "namespace Shop\n{\n    public interface IThing\n    {\n        int Id { get; }\n    }\n}\n"),
    ("src/Thing.cs", "namespace Shop\n{\n    public class Thing : IThing\n    {\n        public int Id { get; set; }\n    }\n}\n"),
    ("src/Island.cs", "namespace Shop\n{\n    public class Island\n    {\n        public int Total() { return 0; }\n    }\n}\n"),
    // A pool of near names one word apart, so a zero-hit query can fill the
    // 5-row suggestion cap and prove which rows the measure drops.
    (
        "src/Toolbar.cs",
        "namespace Shop\n{\n    public class Toolbar\n    {\n        private void PopulateToolbarItems() { }\n\n        private void PopulateToolbarOverflow() { }\n\n        private void PopulateToolbarPins() { }\n\n        private void PopulateToolbarTabs() { }\n\n        private void PopulateToolbarMenus() { }\n    }\n}\n",
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
        let init = fixture.run(&["init", "--no-hooks", "--no-map"]);
        assert!(init.status.success(), "init failed: {init:?}");
        let map = fixture.run(&["map", "."]);
        assert!(map.status.success(), "map failed: {map:?}");
        fixture
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(rust_bin())
            .args(args)
            .current_dir(&self.root)
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

fn stderr_of(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("stderr is utf-8")
}

#[test]
fn every_verb_exits_3_on_a_zero_hit_with_its_own_fixed_line_on_stderr() {
    let fx = Fixture::build("verbs");
    let cases: [(&[&str], &str, &str); 4] = [
        (
            &["find", "zzz123nosuchpurpose"],
            "no matches for \"zzz123nosuchpurpose\" (run 'devscout map' if manifest is missing)\n",
            ZERO_HIT_FIND,
        ),
        (
            &["refs", "ThereIsNoSuchType"],
            "no symbol matches \"ThereIsNoSuchType\"\n",
            ZERO_HIT_REFS,
        ),
        (
            &["impact", "ThereIsNoSuchType"],
            "no symbol match for \"ThereIsNoSuchType\"\n",
            ZERO_HIT_IMPACT,
        ),
        (
            &["tests", "ThereIsNoSuchType"],
            "no symbol matches \"ThereIsNoSuchType\"\n",
            ZERO_HIT_TESTS,
        ),
    ];
    for (args, want_stdout, want_stderr) in cases {
        let out = fx.run(args);
        assert_eq!(out.status.code(), Some(3), "{args:?}: {out:?}");
        assert_eq!(stdout_of(&out), want_stdout, "{args:?}");
        assert_eq!(stderr_of(&out), format!("{want_stderr}\n"), "{args:?}");
    }
}

#[test]
fn a_resolved_seed_that_reaches_nothing_is_a_zero_hit_and_a_reached_one_is_not() {
    let fx = Fixture::build("zero-edges");

    let island = fx.run(&["impact", "src/Island.cs"]);
    assert_eq!(island.status.code(), Some(3), "{island:?}");
    assert!(
        stdout_of(&island).contains("affected files: 0  shown: 0  dropped: 0"),
        "{island:?}"
    );
    assert_eq!(stderr_of(&island), format!("{ZERO_HIT_IMPACT}\n"));

    let reached = fx.run(&["impact", "src/IThing.cs"]);
    assert_eq!(reached.status.code(), Some(0), "{reached:?}");
    assert_eq!(
        stderr_of(&reached),
        "",
        "a non-empty blast radius prints no zero-hit line"
    );
}

#[test]
fn environment_and_usage_failures_keep_their_own_codes_and_print_no_zero_hit_line() {
    let fx = Fixture::build("not-zero-hits");
    let cases: [(&[&str], i32); 3] = [
        (&["refs"], 2),
        (&["impact", "--hops", "zero", "IThing"], 2),
        (&["refs", "IThing", "--compact", "--json"], 1),
    ];
    for (args, want_code) in cases {
        let out = fx.run(args);
        assert_eq!(out.status.code(), Some(want_code), "{args:?}: {out:?}");
        assert_eq!(stderr_of(&out), "", "{args:?}");
    }
}

#[test]
fn a_zero_hit_keeps_the_stdout_shape_under_every_output_flag() {
    let fx = Fixture::build("flags");
    for flag in [None, Some("--compact"), Some("--json")] {
        let mut args = vec!["impact", "src/Island.cs"];
        if let Some(f) = flag {
            args.push(f);
        }
        let out = fx.run(&args);
        assert_eq!(out.status.code(), Some(3), "{args:?}: {out:?}");
        assert!(
            !stdout_of(&out).is_empty(),
            "{args:?}: stdout still carries the rendered answer"
        );
        assert_eq!(stderr_of(&out), format!("{ZERO_HIT_IMPACT}\n"), "{args:?}");
    }
}

// --- The nearest names that ride on the same note ---------------------------
//
// The block is stderr text, so it is asserted here rather than in
// `src/suggest.rs`'s unit tests: what the measure returns and what the CLI
// prints are two different claims, and only the second one is what a caller
// reads.

#[test]
fn a_zero_hit_refs_appends_the_nearest_names_up_to_the_cap() {
    let fx = Fixture::build("suggestions");
    let out = fx.run(&["refs", "PopulateToolbarItem"]);
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    assert_eq!(
        stdout_of(&out),
        "no symbol matches \"PopulateToolbarItem\"\n"
    );
    assert_eq!(
        stderr_of(&out),
        format!(
            "{ZERO_HIT_REFS}\ndid you mean:\n\
             \x20 PopulateToolbarItems  method  src/Toolbar.cs:5\n\
             \x20 Toolbar  class  src/Toolbar.cs:3\n\
             \x20 PopulateToolbarOverflow  method  src/Toolbar.cs:7\n\
             \x20 PopulateToolbarPins  method  src/Toolbar.cs:9\n\
             \x20 PopulateToolbarTabs  method  src/Toolbar.cs:11\n"
        ),
    );
}

#[test]
fn a_zero_hit_find_appends_the_same_block_and_stops_below_the_cap_when_the_index_does() {
    let fx = Fixture::build("suggestions-find");
    let out = fx.run(&["find", "Islnad"]);
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    assert_eq!(
        stdout_of(&out),
        "no matches for \"Islnad\" (run 'devscout map' if manifest is missing)\n"
    );
    assert_eq!(
        stderr_of(&out),
        format!("{ZERO_HIT_FIND}\ndid you mean:\n  Island  class  src/Island.cs:3\n")
    );
}

#[test]
fn a_zero_hit_near_nothing_prints_the_fixed_line_alone_with_no_trailing_blank() {
    let fx = Fixture::build("suggestions-none");
    let out = fx.run(&["refs", "ThereIsNoSuchType"]);
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    assert_eq!(stderr_of(&out), format!("{ZERO_HIT_REFS}\n"));
}

#[test]
fn impact_and_tests_keep_the_fixed_line_alone_even_beside_a_near_name() {
    let fx = Fixture::build("suggestions-other-verbs");
    let impact = fx.run(&["impact", "PopulateToolbarItem"]);
    assert_eq!(impact.status.code(), Some(3), "{impact:?}");
    assert_eq!(stderr_of(&impact), format!("{ZERO_HIT_IMPACT}\n"));

    let tests = fx.run(&["tests", "PopulateToolbarItem"]);
    assert_eq!(tests.status.code(), Some(3), "{tests:?}");
    assert_eq!(stderr_of(&tests), format!("{ZERO_HIT_TESTS}\n"));
}
