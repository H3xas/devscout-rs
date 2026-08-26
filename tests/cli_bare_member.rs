//! Integration tests for bare-member command-line queries.

// Bare member names: `refs <member>` names the declaring type through the
// member index and then keeps only the inbound member edges whose own line
// carries the member as a whole token.
//
// Every test here goes through the COMPILED BINARY as a subprocess against a
// graph the binary itself mapped, because the whole point of the verification
// step is that it reads real source lines off disk -- an in-process model call
// could pass with a fixture that never existed as a file.
//
// The expected stdout is pinned here as exact literals over the two fixture
// files below -- the same device cli_zero_hit.rs uses for its four stderr
// lines: the rendering is part of the contract, so changing its wording has to
// be a deliberate edit to these tests.
//
// SAFETY: every spawned process gets HOME, SCOUT_REGISTRY and SCOUT_CONTENT_DB
// pointed at a fresh temp dir, so the operator's real settings and registry are
// never read and never written.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-bare-member-{prefix}-{}-{n}",
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

/// `Post` and `PostEx` are declared two lines apart and called two lines apart,
/// which is what makes a substring match and a token match give visibly
/// different answers. `Reconcile` is declared on the same type and named on no
/// edge line at all.
const FILES: &[(&str, &str)] = &[
    (
        "src/Ledger.cs",
        "namespace App.Books\n{\n    public static class Ledger\n    {\n        public static void Post(int amount) { }\n\n        public static void PostEx(int amount) { }\n\n        private static void Reconcile() { }\n    }\n}\n",
    ),
    (
        "src/Consumer.cs",
        "using App.Books;\n\nnamespace App.Books\n{\n    public class Consumer\n    {\n        public void Run()\n        {\n            Ledger.Post(1);\n            Ledger.PostEx(2);\n        }\n    }\n}\n",
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

#[test]
fn a_bare_member_answers_with_its_declaring_type_and_only_the_lines_that_name_it() {
    let fx = Fixture::build("unique");
    let out = fx.run(&["refs", "PostEx"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(
        stdout_of(&out),
        "App.Books.Ledger.PostEx  (member)\n\
         def: src/Ledger.cs:7\n\
         inbound:\n\
         \x20 inherits (0):\n\
         \x20 uses-type (0):\n\
         \x20 uses-member (1):\n\
         \x20   src/Consumer.cs:10  uses-member  Ledger.PostEx(2);\n"
    );
}

#[test]
fn a_bare_member_never_matches_a_longer_identifier_that_starts_with_it() {
    let fx = Fixture::build("token");
    let out = fx.run(&["refs", "Post"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = stdout_of(&out);
    assert!(
        text.contains("src/Consumer.cs:9  uses-member  Ledger.Post(1);"),
        "{text}"
    );
    assert!(
        !text.contains("PostEx"),
        "line 10 is a substring hit that must not survive verification: {text}"
    );
}

#[test]
fn a_member_no_edge_line_names_keeps_the_unchanged_zero_hit_exit() {
    let fx = Fixture::build("unverified");
    let out = fx.run(&["refs", "Reconcile"]);
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    assert_eq!(stdout_of(&out), "no symbol matches \"Reconcile\"\n");
}

// `Approve` is a static method declared on two independent classes, each with
// its own inbound call site, so both survive the same edge-line verification
// `PostEx`/`Post` above rely on. More than one declaring type surviving is an
// ambiguity `refs` refuses to guess through: the answer is the exact candidate
// list an ambiguous TYPE name already renders (id, def site, kind; sorted;
// exit 1), never a per-type `uses-member` block.
const AMBIGUOUS_FILES: &[(&str, &str)] = &[
    (
        "src/Ledger.cs",
        "namespace App.Books\n{\n    public static class Ledger\n    {\n        public static void Approve(int amount) { }\n    }\n}\n",
    ),
    (
        "src/Journal.cs",
        "namespace App.Books\n{\n    public static class Journal\n    {\n        public static void Approve(int amount) { }\n    }\n}\n",
    ),
    (
        "src/Consumer.cs",
        "using App.Books;\n\nnamespace App.Books\n{\n    public class Consumer\n    {\n        public void Run()\n        {\n            Ledger.Approve(1);\n            Journal.Approve(2);\n        }\n    }\n}\n",
    ),
];

fn ambiguous_member_fixture(prefix: &str) -> Fixture {
    let base = temp_dir(prefix);
    let root = base.join("repo");
    let home = base.join("home");
    fs::create_dir_all(&home).expect("create home dir");
    for (rel, body) in AMBIGUOUS_FILES {
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

const AMBIGUOUS_APPROVE_OUT: &str = "ambiguous symbol \"Approve\" — 2 candidates:\n\
                                      App.Books.Journal  src/Journal.cs:3  class\n\
                                      App.Books.Ledger  src/Ledger.cs:3  class\n";

#[test]
fn a_bare_member_declared_on_two_types_renders_the_ambiguous_candidate_list_never_a_members_block()
{
    let fx = ambiguous_member_fixture("ambiguous");
    let out = fx.run(&["refs", "Approve"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert_eq!(stdout_of(&out), AMBIGUOUS_APPROVE_OUT);
}

#[test]
fn an_ambiguous_bare_member_ignores_json_and_compact_same_as_an_ambiguous_type_name() {
    let fx = ambiguous_member_fixture("ambiguous-flags");
    let json = fx.run(&["refs", "Approve", "--json"]);
    assert_eq!(stdout_of(&json), AMBIGUOUS_APPROVE_OUT);
    let compact = fx.run(&["refs", "Approve", "--compact"]);
    assert_eq!(stdout_of(&compact), AMBIGUOUS_APPROVE_OUT);
}
