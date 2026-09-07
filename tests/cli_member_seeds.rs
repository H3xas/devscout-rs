//! Integration coverage for `fixtures/member-seeds/`: the four situations a
//! member seed can put `refs`/`read`/`impact`/`tests` in, one per `outcome`
//! value (see the fixture's own README for the case table).
//!
//! Every test here goes through the COMPILED BINARY as a subprocess against a
//! graph the binary itself mapped, the same device `cli_bare_member.rs` uses:
//! resolution reads real source text off disk, so an in-process model call
//! could not exercise the same path.
//!
//! SAFETY: every spawned process gets HOME, SCOUT_REGISTRY and
//! SCOUT_CONTENT_DB pointed at a fresh temp dir, so the operator's real
//! settings and registry are never read and never written.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-member-seeds-{prefix}-{}-{n}",
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

const FIXTURE_FILES: &[&str] = &["Galley.cs", "Larder.cs", "Anchor.cs", "Quartermaster.cs"];

struct Fixture {
    root: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn build() -> Fixture {
        let base = temp_dir("fixture");
        let root = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&root).expect("create repo dir");
        fs::create_dir_all(&home).expect("create home dir");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/member-seeds");
        for name in FIXTURE_FILES {
            fs::copy(source.join(name), root.join(name)).expect("copy fixture file");
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

// --- `hit`: a unique bare member name ---------------------------------------

#[test]
fn a_unique_bare_member_name_answers_hit_on_every_verb() {
    let fx = Fixture::build();

    let refs = fx.run(&["refs", "Ladle", "--json"]);
    assert_eq!(refs.status.code(), Some(0), "{refs:?}");
    assert!(stdout_of(&refs).contains(r#""outcome":"hit""#), "{refs:?}");

    let read = fx.run(&["read", "Ladle", "--json"]);
    assert_eq!(read.status.code(), Some(0), "{read:?}");
    assert!(stdout_of(&read).contains(r#""outcome":"hit""#), "{read:?}");

    let impact = fx.run(&["impact", "Ladle", "--json"]);
    assert_eq!(impact.status.code(), Some(0), "{impact:?}");
    assert!(
        stdout_of(&impact).contains(r#""outcome":"hit""#),
        "{impact:?}"
    );

    let tests = fx.run(&["tests", "Ladle", "--json"]);
    assert_eq!(tests.status.code(), Some(0), "{tests:?}");
    assert!(
        stdout_of(&tests).contains(r#""outcome":"hit""#),
        "{tests:?}"
    );
}

// --- `ambiguous`: a member name carried by two types -------------------------

#[test]
fn a_member_name_carried_by_two_types_answers_ambiguous_with_candidate_rows() {
    let fx = Fixture::build();

    let refs = fx.run(&["refs", "Stow", "--json"]);
    assert_eq!(refs.status.code(), Some(1), "{refs:?}");
    let out = stdout_of(&refs);
    assert!(out.contains(r#""outcome":"ambiguous""#), "{out}");
    assert!(out.contains("Nautical.Crew.Galley"), "{out}");
    assert!(out.contains("Nautical.Crew.Larder"), "{out}");

    match fx.run(&["impact", "Stow"]).status.code() {
        Some(1) => {}
        other => panic!("impact must refuse to guess between two owners: {other:?}"),
    }
    match fx.run(&["tests", "Stow"]).status.code() {
        Some(1) => {}
        other => panic!("tests must refuse to guess between two owners: {other:?}"),
    }
}

// --- `zero-hit`: a `Type.Member` spelling naming an unreferenced member -----

#[test]
fn a_type_dot_member_seed_resolves_but_reaches_nothing() {
    let fx = Fixture::build();

    let impact = fx.run(&["impact", "Anchor.Weigh", "--hops", "1", "--json"]);
    assert_eq!(impact.status.code(), Some(3), "{impact:?}");
    let out = stdout_of(&impact);
    assert!(out.contains(r#""outcome":"zero-hit""#), "{out}");
    assert!(out.contains(r#""rows":[]"#), "{out}");
}

// --- `fallback-advised`: a name the graph does not hold at all --------------

#[test]
fn a_name_the_graph_does_not_hold_advises_a_text_search_fallback() {
    let fx = Fixture::build();

    for args in [
        vec!["refs", "Boatswain", "--json"],
        vec!["read", "Boatswain", "--json"],
        vec!["impact", "Boatswain", "--json"],
        vec!["tests", "Boatswain", "--json"],
    ] {
        let out = fx.run(&args);
        assert_eq!(out.status.code(), Some(3), "{args:?}: {out:?}");
        let stdout = stdout_of(&out);
        assert!(
            stdout.contains(r#""outcome":"fallback-advised""#),
            "{args:?}: {stdout}"
        );
    }
}

// --- `--pick`, exercised over the same ambiguous `Stow` seed ----------------

#[test]
fn pick_narrows_an_ambiguous_member_seed_to_one_candidate() {
    let fx = Fixture::build();

    let picked = fx.run(&["refs", "Stow", "--pick", "1", "--json"]);
    assert_eq!(picked.status.code(), Some(0), "{picked:?}");
    let out = stdout_of(&picked);
    assert!(out.contains(r#""outcome":"hit""#), "{out}");
    assert!(
        out.contains(r#""id":"Nautical.Crew.Galley.Stow""#),
        "candidate 1 is name-index order's first owner: {out}"
    );

    let out_of_range = fx.run(&["refs", "Stow", "--pick", "99"]);
    assert_eq!(out_of_range.status.code(), Some(2), "{out_of_range:?}");
}
