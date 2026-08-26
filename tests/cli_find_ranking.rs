//! Integration tests for command-line find-result ranking.

// Ranked `find`: whichever manifest pool answers is sorted by tokens matched,
// then by the file's precise inbound-edge count, BEFORE the cap -- so on a text
// tie the widely referenced file prints above the island, and a repo with no
// graph file answers in plain manifest order, byte for byte as before.
//
// Every test goes through the COMPILED BINARY as a subprocess (the stream split
// and the full map pipeline are half of what is under test), following
// tests/cli_zero_hit.rs's fixture shape.
//
// SAFETY: every spawned process gets HOME, SCOUT_REGISTRY and SCOUT_CONTENT_DB
// pointed at a fresh temp dir, so the operator's real ~/.claude/settings.json
// and repos.json are never read and never written.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-find-ranking-{prefix}-{}-{n}",
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

// The token-tie pair. Both leading comments carry BOTH query tokens ("gadget
// ledger"), so the AND pool's primary key is constant and the inbound
// tie-break alone decides. `AaLedger` deliberately sorts FIRST in manifest
// order while carrying zero references, so the ranked order and the on-disk
// order disagree -- the only shape that can tell the two apart.
const ISLAND_CS: &str = "// gadget ledger store\nnamespace Shop;\n\npublic class AaLedger\n{\n    public int Total { get; set; }\n}\n";
const HUB_CS: &str = "// gadget ledger hub\nnamespace Shop;\n\npublic class BbLedger\n{\n    public int Count { get; set; }\n}\n";

// Two consumers of `BbLedger`, each contributing a precise `uses-type` edge
// landing on src/BbLedger.cs. Their comments carry neither query token, so
// neither joins either pool.
const CONSUMER_ONE_CS: &str =
    "// just a consumer\nnamespace Shop;\n\npublic class ConsumerOne\n{\n    public BbLedger Ledger { get; set; }\n}\n";
const CONSUMER_TWO_CS: &str =
    "// another consumer\nnamespace Shop;\n\npublic class ConsumerTwo\n{\n    public BbLedger Holder { get; set; }\n}\n";

const FILES: &[(&str, &str)] = &[
    ("src/AaLedger.cs", ISLAND_CS),
    ("src/BbLedger.cs", HUB_CS),
    ("src/ConsumerOne.cs", CONSUMER_ONE_CS),
    ("src/ConsumerTwo.cs", CONSUMER_TWO_CS),
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

    /// The shared artifact location git resolves for this root, plus the graph
    /// file under it.
    fn scout_dir(&self) -> PathBuf {
        let out = Command::new("git")
            .args(["rev-parse", "--git-common-dir"])
            .current_dir(&self.root)
            .output()
            .expect("git rev-parse must run");
        assert!(
            out.status.success(),
            "git rev-parse --git-common-dir failed: {out:?}"
        );
        let common = String::from_utf8(out.stdout).unwrap().trim().to_string();
        let common_path =
            fs::canonicalize(self.root.join(common)).expect("canonicalize common dir");
        common_path.join("scout")
    }
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

#[test]
fn find_ranks_a_token_tie_by_inbound_references_before_the_cap() {
    let fx = Fixture::build("ranked");
    // Sanity: the fixture really produced precise inbound edges for the hub --
    // without them the ordering assertion below would pass vacuously on
    // manifest order.
    let refs = fx.run(&["refs", "BbLedger"]);
    assert_eq!(refs.status.code(), Some(0), "{refs:?}");
    let refs_out = String::from_utf8(refs.stdout.clone()).unwrap();
    assert!(
        refs_out.contains("uses-type (2):"),
        "fixture must carry two precise uses-type edges: {refs_out}"
    );

    let out = fx.run(&["find", "gadget ledger"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let stdout = stdout_of(&out);
    let lines: Vec<&str> = stdout.split('\n').filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines.len(),
        2,
        "only the two token-tied entries answer: {stdout}"
    );
    assert!(
        lines[0].starts_with("src/BbLedger.cs:") && lines[1].starts_with("src/AaLedger.cs:"),
        "the referenced file outranks the manifest-order-first island on the text tie: {stdout}"
    );
}

#[test]
fn find_on_a_repo_with_no_graph_file_prints_unchanged_manifest_order() {
    let fx = Fixture::build("no-graph");
    let graph = fx.scout_dir().join("graph").join("graph.json");
    assert!(
        graph.exists(),
        "the mapped fixture must have a graph to delete"
    );
    fs::remove_file(&graph).expect("delete graph.json");

    let out = fx.run(&["find", "gadget ledger"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let stdout = stdout_of(&out);
    let lines: Vec<&str> = stdout.split('\n').filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 2, "{stdout}");
    assert!(
        lines[0].starts_with("src/AaLedger.cs:") && lines[1].starts_with("src/BbLedger.cs:"),
        "no graph, no ranking: manifest on-disk order stands: {stdout}"
    );
}
