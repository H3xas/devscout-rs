//! Integration tests for command-line repository-root handling.

// Root resolution that does not depend on the caller's working directory: the
// global `-C <dir>` flag, and `impact`'s fallback to the root of its own path
// argument.
//
// Every test here goes through the COMPILED BINARY as a subprocess, so
// nothing in this file mutates process-global state and no lock is needed --
// same rule as tests/init_full.rs, whose helpers below are duplicated
// deliberately (each tests/*.rs file compiles as an independent binary).
//
// SAFETY: every spawned process gets HOME, SCOUT_REGISTRY and
// SCOUT_CONTENT_DB pointed at a fresh temp dir, so the operator's real
// ~/.claude/settings.json and repos.json are never read and never written.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-cli-root-{prefix}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    // macOS's `env::temp_dir()` is itself a symlink (`/var` -> `/private/var`)
    // and a child process reports the canonical form as its cwd, so a `-C`
    // argument spelled the other way would resolve the same root under a
    // different NAME. Canonicalizing once here keeps the two spellings equal,
    // which is what makes the byte-identity assertions below meaningful.
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

struct Fixture {
    root: PathBuf,
    outside: PathBuf,
    home: PathBuf,
}

const FILES: &[(&str, &str)] = &[
    ("src/IThing.cs", "namespace Shop\n{\n    public interface IThing\n    {\n        int Id { get; }\n    }\n}\n"),
    ("src/Thing.cs", "namespace Shop\n{\n    public class Thing : IThing\n    {\n        public int Id { get; set; }\n    }\n}\n"),
    (
        "tests/ThingTests.cs",
        "using NUnit.Framework;\n\nnamespace Shop.Tests\n{\n    public class ThingTests\n    {\n        [Test]\n        public void Reads() { var t = new Thing(); }\n    }\n}\n",
    ),
];

impl Fixture {
    /// One git repo carrying a two-type C# graph, `devscout init`-ed and mapped,
    /// plus a sibling directory with neither `.scout` nor `.git` above it --
    /// the shape a caller whose shell starts somewhere else lands in.
    fn build(prefix: &str) -> Fixture {
        let base = temp_dir(prefix);
        let root = base.join("repo");
        let outside = base.join("outside");
        let home = base.join("home");
        fs::create_dir_all(&outside).expect("create outside dir");
        fs::create_dir_all(&home).expect("create home dir");
        for (rel, body) in FILES {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).expect("create fixture dir");
            fs::write(&path, body).expect("write fixture file");
        }
        run_git(&root, &["init", "-q", "."]);
        bootstrap_initial_commit(&root);
        let fixture = Fixture {
            root,
            outside,
            home,
        };
        fixture.expect_ok(&fixture.root, &["init", "--no-hooks", "--no-map"]);
        fixture.expect_ok(&fixture.root, &["map", "."]);
        // A second map so every later `map` compares one no-change run
        // against another, rather than a cold run against a warm one.
        fixture.expect_ok(&fixture.root, &["map", "."]);
        fixture
    }

    fn run(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(rust_bin())
            .args(args)
            .current_dir(cwd)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .output()
            .expect("devscout must run")
    }

    fn expect_ok(&self, cwd: &Path, args: &[&str]) -> String {
        let out = self.run(cwd, args);
        assert!(out.status.success(), "devscout {args:?} failed: {out:?}");
        String::from_utf8(out.stdout).expect("stdout is utf-8")
    }

    fn root_str(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

#[test]
fn dash_c_answers_every_root_resolving_verb_byte_identically_from_an_unrelated_cwd() {
    let fx = Fixture::build("dash-c");
    let root = fx.root_str();
    let matrix: &[&[&str]] = &[
        &["find", "thing"],
        &["refs", "IThing"],
        &["refs", "IThing", "--compact"],
        &["refs", "IThing", "--json"],
        &["impact", "src/IThing.cs"],
        &["impact", "IThing", "--hops", "1", "--compact"],
        &["tests", "Thing"],
        &["tests", "Thing", "--json"],
        &["map", "."],
    ];
    for args in matrix {
        let inside = fx.run(&fx.root, args);
        let mut with_flag: Vec<&str> = vec!["-C", &root];
        with_flag.extend_from_slice(args);
        let outside = fx.run(&fx.outside, &with_flag);
        assert_eq!(
            stdout_of(&inside),
            stdout_of(&outside),
            "stdout differs for {args:?}"
        );
        assert_eq!(inside.stderr, outside.stderr, "stderr differs for {args:?}");
        assert_eq!(
            inside.status.code(),
            outside.status.code(),
            "exit code differs for {args:?}"
        );
        assert_eq!(
            inside.status.code(),
            Some(0),
            "{args:?} did not answer from inside the repo either"
        );
    }
}

#[test]
fn dash_c_reaches_stats_and_init_too() {
    let fx = Fixture::build("dash-c-write");
    let root = fx.root_str();
    let stats = fx.expect_ok(&fx.outside, &["-C", &root, "stats"]);
    assert!(
        stats.starts_with(&format!("devscout stats ({root}):")),
        "stats named a different root: {stats}"
    );

    let sub = fx.root.join("sub");
    fs::create_dir_all(&sub).expect("create sub dir");
    let out = fx.expect_ok(
        &fx.outside,
        &[
            "-C",
            &sub.to_string_lossy(),
            "init",
            "--no-hooks",
            "--no-map",
        ],
    );
    // `-C` decides the directory init resolves FROM, not what init does with
    // it: the `.git` ancestor still wins over the subdirectory named.
    assert!(
        out.contains(&fx.root.join(".scout").to_string_lossy().into_owned()),
        "init resolved elsewhere: {out}"
    );
    assert!(!sub.join(".scout").exists());
    assert!(!fx.outside.join(".scout").exists());
}

#[test]
fn dash_c_composes_on_repeat_like_git() {
    let fx = Fixture::build("dash-c-compose");
    let parent = fx
        .root
        .parent()
        .expect("fixture root has a parent")
        .to_string_lossy()
        .into_owned();
    let direct = fx.expect_ok(&fx.root, &["refs", "IThing", "--compact"]);
    let composed = fx.expect_ok(
        &fx.outside,
        &["-C", &parent, "-C", "repo", "refs", "IThing", "--compact"],
    );
    assert_eq!(direct, composed);
}

#[test]
fn dash_c_rejects_a_missing_or_nonexistent_directory() {
    let fx = Fixture::build("dash-c-bad");
    let missing = fx.run(&fx.outside, &["-C"]);
    assert_eq!(missing.status.code(), Some(1));
    assert_eq!(
        stdout_of(&missing),
        "error: no directory given for '-C' option\n"
    );

    let nonexistent = fx.run(&fx.outside, &["-C", "nope", "find", "thing"]);
    assert_eq!(nonexistent.status.code(), Some(1));
    assert_eq!(
        stdout_of(&nonexistent),
        "error: cannot change to 'nope': no such directory\n"
    );
}

#[test]
fn impact_derives_the_root_from_an_absolute_path_argument_with_no_flag() {
    let fx = Fixture::build("arg-abs");
    let seed = fx.root.join("src/IThing.cs");
    let inside = fx.expect_ok(&fx.root, &["impact", "src/IThing.cs", "--compact"]);
    let outside = fx.expect_ok(
        &fx.outside,
        &["impact", &seed.to_string_lossy(), "--compact"],
    );
    assert_eq!(inside, outside);
}

#[test]
fn impact_reads_a_relative_path_argument_from_the_directory_it_was_given() {
    let fx = Fixture::build("arg-rel");
    let inside = fx.expect_ok(&fx.root, &["impact", "src/IThing.cs", "--compact"]);
    // Same file, named from a subdirectory of the repo and from `-C`'s
    // directory: both rewrite to the repo-relative form the manifest keys on.
    let from_sub = fx.expect_ok(&fx.root.join("src"), &["impact", "IThing.cs", "--compact"]);
    let from_flag = fx.expect_ok(
        &fx.outside,
        &["-C", &fx.root_str(), "impact", "src/IThing.cs", "--compact"],
    );
    assert_eq!(inside, from_sub);
    assert_eq!(inside, from_flag);
}

#[test]
fn the_callers_directory_wins_over_the_argument_path_unless_dash_c_says_otherwise() {
    let a = Fixture::build("two-repos-a");
    let b = Fixture::build("two-repos-b");
    let seed_in_b = b.root.join("src/IThing.cs");
    let seed_str = seed_in_b.to_string_lossy().into_owned();

    // Standing in repo A, an argument pointing into repo B is answered
    // against A -- where that path is not a file the manifest knows.
    let from_a = a.run(&a.root, &["impact", &seed_str, "--compact"]);
    // A path the manifest does not know is a zero hit, not an error.
    assert_eq!(from_a.status.code(), Some(3));
    assert_eq!(
        stdout_of(&from_a),
        format!("no file match for \"{seed_str}\"\n")
    );

    // The same argument with `-C` naming repo B resolves there instead.
    let inside_b = b.expect_ok(&b.root, &["impact", "src/IThing.cs", "--compact"]);
    let via_flag = a.expect_ok(
        &a.root,
        &[
            "-C",
            &b.root.to_string_lossy(),
            "impact",
            &seed_str,
            "--compact",
        ],
    );
    assert_eq!(inside_b, via_flag);
}

#[test]
fn a_symbol_argument_never_decides_the_root_and_the_error_text_is_unchanged() {
    let fx = Fixture::build("no-root");
    for args in [
        &["refs", "IThing"][..],
        &["tests", "IThing"][..],
        &["impact", "IThing"][..],
        &["find", "thing"][..],
    ] {
        let out = fx.run(&fx.outside, args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(
            stdout_of(&out),
            "error: no .scout or .git ancestor; run 'devscout init' from the repo or directory root\n",
            "{args:?}",
        );
    }
    // A path-shaped argument that names nothing on disk is refused the same
    // way: the fallback needs a real path, not a repo-relative guess.
    let out = fx.run(&fx.outside, &["impact", "src/IThing.cs"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stdout_of(&out),
        "error: no .scout or .git ancestor; run 'devscout init' from the repo or directory root\n"
    );
}
