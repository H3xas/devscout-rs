// The `read` verb: declaration span (start AND end line) plus the same
// capped-and-ranked inbound answer `refs` gives, with refs' own ambiguity and
// zero-hit discipline.
//
// Every test here goes through the COMPILED BINARY as a subprocess -- the
// stdout/stderr split is half of what is under test (the zero-hit note lives
// on stderr, the answer on stdout), which an in-process `cmd_*` call cannot
// tell apart. Same rule as tests/cli_zero_hit.rs, whose helpers below are
// duplicated deliberately (each tests/*.rs file compiles as an independent
// binary).
//
// SAFETY: every spawned process gets HOME, SCOUT_REGISTRY and SCOUT_CONTENT_DB
// pointed at a fresh temp dir, so the operator's real ~/.claude/settings.json
// and repos.json are never read and never written.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

/// Spelled out here rather than imported from `src/cli.rs`: an integration
/// test asserts what a caller actually reads off stderr, so a silent edit to
/// the wording upstream has to fail here.
const ZERO_HIT_READ: &str = "devscout read: zero hits — the graph was searched and no symbol matched. Not an error; fall back to text search (rg/grep) rather than rephrasing.";

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-cli-read-{prefix}-{}-{n}",
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

// Line-numbered shapes, so the span assertions can name exact bounds:
//
// src/IThing.cs                src/Ledger/Item.cs      src/Warehouse/Item.cs
// 1 namespace Shop             1 namespace Shop.Ledger 1 namespace Shop.Warehouse
// 2 {                          2 {                     2 {
// 3     public interface ...   3     public class Item 3     public class Item
// 4     {                      4     {                 4     {
// 5         int Id { get; }    5     }                 5     }
// 6     }                      6 }                     6 }
// 7 }
const FILES: &[(&str, &str)] = &[
    ("src/IThing.cs", "namespace Shop\n{\n    public interface IThing\n    {\n        int Id { get; }\n    }\n}\n"),
    ("src/Thing.cs", "namespace Shop\n{\n    public class Thing : IThing\n    {\n        public int Id { get; set; }\n    }\n}\n"),
    // Names the `Id` member on a real edge line, which is what lets the
    // bare-member fallback answer `read Id` at all (a member no edge names is
    // a zero hit -- see tests/cli_bare_member.rs).
    ("src/Consumer.cs", "using Shop;\n\nnamespace Shop.Consumers\n{\n    public class Consumer\n    {\n        public int Get(Thing t)\n        {\n            return t.Id;\n        }\n    }\n}\n"),
    ("src/Ledger/Item.cs", "namespace Shop.Ledger\n{\n    public class Item\n    {\n    }\n}\n"),
    ("src/Warehouse/Item.cs", "namespace Shop.Warehouse\n{\n    public class Item\n    {\n    }\n}\n"),
    ("src/widget.ts", "export interface Widget {\n  id: number;\n  label: string;\n}\n"),
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

    // The artifact tree lives under the git common dir even in a plain
    // single-worktree clone -- this is where the fragments cache generation
    // files are asserted below.
    fn graph_dir(&self) -> PathBuf {
        self.root.join(".git").join("scout").join("graph")
    }
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("stderr is utf-8")
}

#[test]
fn read_prints_the_declaration_span_with_verbatim_source_and_inbound() {
    let fx = Fixture::build("span");
    let out = fx.run(&["read", "IThing"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(
        stdout_of(&out),
        concat!(
            "Shop.IThing  (interface)\n",
            "def: src/IThing.cs:3-6\n",
            "    public interface IThing\n",
            "    {\n",
            "        int Id { get; }\n",
            "    }\n",
            "inbound:\n",
            "  inherits (1):\n",
            "    src/Thing.cs:3  inherits  public class Thing : IThing\n",
            "  uses-type (0):\n",
            "  uses-member (0):\n",
        ),
        "span bounds 3-6 with the source quoted verbatim, then refs' inbound block"
    );
    assert_eq!(stderr_of(&out), "");
}

#[test]
fn read_json_carries_span_file_bounds_and_source() {
    let fx = Fixture::build("json");
    let out = fx.run(&["read", "IThing", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let v: serde_json::Value = serde_json::from_str(stdout_of(&out).trim()).expect("valid JSON");
    assert_eq!(v["status"], "resolved");
    assert_eq!(v["id"], "Shop.IThing");
    assert_eq!(v["span"]["file"], "src/IThing.cs");
    assert_eq!(v["span"]["startLine"], 3);
    assert_eq!(v["span"]["endLine"], 6);
    assert_eq!(
        v["span"]["source"], "    public interface IThing\n    {\n        int Id { get; }\n    }",
        "the source is the whole declaration verbatim, indentation included"
    );
    assert_eq!(v["sites"][0]["line"], 3);
    assert_eq!(
        v["inbound"]["inherits"]["total"], 1,
        "inbound rides on the same machinery as refs"
    );
    assert_eq!(v["manifestGap"], 0);
}

#[test]
fn read_json_carries_the_real_typescript_declaration_span() {
    let fx = Fixture::build("ts-span");
    let out = fx.run(&["read", "Widget", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&out)).unwrap();
    assert_eq!(v["span"]["file"], "src/widget.ts");
    assert_eq!(v["span"]["startLine"], 1);
    assert_eq!(v["span"]["endLine"], 4);
    assert_eq!(
        v["span"]["source"],
        "export interface Widget {\n  id: number;\n  label: string;\n}"
    );
}

#[test]
fn read_compact_keeps_the_span_on_the_def_line_and_drops_the_snippets() {
    let fx = Fixture::build("compact");
    let out = fx.run(&["read", "IThing", "--compact"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let stdout = stdout_of(&out);
    assert!(
        stdout.starts_with("Shop.IThing  (interface)\ndef: src/IThing.cs:3-6\n"),
        "{stdout:?}"
    );
    assert!(
        stdout.contains("in:inherits (1):\n  src/Thing.cs:3"),
        "{stdout:?}"
    );
    assert!(
        !stdout.contains("public class Thing : IThing"),
        "compact carries no per-hit source: {stdout:?}"
    );
}

#[test]
fn an_ambiguous_name_prints_every_candidate_and_exits_1() {
    let fx = Fixture::build("ambiguous");
    let out = fx.run(&["read", "Item"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert_eq!(
        stdout_of(&out),
        concat!(
            "ambiguous symbol \"Item\" — 2 candidates:\n",
            "Shop.Ledger.Item  src/Ledger/Item.cs:3  class\n",
            "Shop.Warehouse.Item  src/Warehouse/Item.cs:3  class\n",
        ),
        "every candidate printed, none picked"
    );
    assert_eq!(
        stderr_of(&out),
        "",
        "an ambiguity is not a zero hit and prints no zero-hit note"
    );
}

#[test]
fn a_zero_hit_exits_3_with_the_verbs_own_fixed_line_on_stderr() {
    let fx = Fixture::build("zero-hit");
    let out = fx.run(&["read", "ThereIsNoSuchType"]);
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    assert_eq!(stdout_of(&out), "no symbol matches \"ThereIsNoSuchType\"\n");
    assert_eq!(stderr_of(&out), format!("{ZERO_HIT_READ}\n"));
}

#[test]
fn usage_and_flag_conflicts_keep_their_own_codes_and_silence() {
    let fx = Fixture::build("usage");
    let missing = fx.run(&["read"]);
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
    assert_eq!(
        stdout_of(&missing),
        "usage: devscout read <symbol> [--json|--compact]\n"
    );

    let conflict = fx.run(&["read", "IThing", "--compact", "--json"]);
    assert_eq!(conflict.status.code(), Some(1), "{conflict:?}");
    assert_eq!(
        stdout_of(&conflict),
        "devscout read: --compact and --json are mutually exclusive\n"
    );

    for bad in [&missing, &conflict] {
        assert_eq!(stderr_of(bad), "", "{bad:?}");
    }
}

#[test]
fn a_bare_member_answers_through_refs_shape_without_claiming_a_span() {
    let fx = Fixture::build("member");
    // `read` resolves exactly as `refs` does, so a bare member falls through
    // to the same member answer (`Type.Member`, kind "member") rendered by
    // refs' own renderer -- nothing records a member end line, so the def
    // line stays start-only.
    let out = fx.run(&["read", "Id"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let stdout = stdout_of(&out);
    assert_eq!(
        stdout,
        concat!(
            "Shop.Thing.Id  (member)\n",
            "def: src/Thing.cs:5\n",
            "inbound:\n",
            "  inherits (0):\n",
            "  uses-type (0):\n",
            "  uses-member (1):\n",
            "    src/Consumer.cs:9  uses-member  return t.Id;\n",
        ),
        "the member answer carries no span range anywhere"
    );
}

#[test]
fn map_rerun_reuses_fragments_and_a_missing_cache_re_extracts_with_spans() {
    let fx = Fixture::build("reuse");
    let rerun = fx.run(&["map", "."]);
    assert!(rerun.status.success(), "{rerun:?}");
    assert!(
        stdout_of(&rerun).contains("0 new, 0 removed"),
        "an unchanged rerun must reuse, not re-extract: {:?}",
        stdout_of(&rerun)
    );

    let graph_dir = fx.graph_dir();
    let v14 = graph_dir.join("fragments-v14.json");
    assert!(v14.exists(), "the current cache generation is on disk");

    // Simulate the pre-bump world: only a v13 pair present. BOTH v14 files
    // must go -- reuse is decided against the mtime-only index, so leaving it
    // behind would let every file look reusable off an empty payload cache.
    // The next map then finds nothing reusable, re-extracts every file,
    // writes the v14 pair again, and deletes the superseded generation.
    fs::write(graph_dir.join("fragments-v13.json"), b"{}").unwrap();
    fs::remove_file(&v14).unwrap();
    fs::remove_file(graph_dir.join("fragments-index-v14.json")).unwrap();

    let rebuild = fx.run(&["map", "."]);
    assert!(rebuild.status.success(), "{rebuild:?}");
    assert!(v14.exists(), "re-extraction rewrote the current generation");
    assert!(
        !graph_dir.join("fragments-v13.json").exists(),
        "rename IS the invalidation: superseded generations are deleted"
    );

    let read = fx.run(&["read", "IThing", "--json"]);
    assert_eq!(read.status.code(), Some(0), "{read:?}");
    let v: serde_json::Value = serde_json::from_str(stdout_of(&read).trim()).expect("valid JSON");
    assert_eq!(
        v["span"]["endLine"], 6,
        "the re-extracted fragment carries end lines again"
    );
}
