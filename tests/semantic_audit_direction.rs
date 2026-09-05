//! Integration test for `devscout audit --semantic` over the base/interface
//! DIRECTION fixture (`fixtures/csharp-direction`).
//!
//! The fixture probes every shape where the compiler's answer to "which type
//! declares this member" runs along an `inherits` edge: inherited, overridden
//! and hidden members through derived, base-typed, `this.` and `base.`
//! receivers; interface-typed receivers over implicit and explicit
//! implementations and over a base interface; static members named through a
//! bare, qualified or generic derived type; declared-type-versus-initializer
//! locals, fields and properties; and the accessibility shapes the resolver
//! cannot yet see. `oracle/refs.jsonl` and `oracle/units.jsonl` are committed,
//! Roslyn-derived ground truth (`tools/scout-semantic` over `Direction.sln`,
//! never invoked here); this file only checks that `devscout`'s own graph
//! still scores against them exactly as documented.
//!
//! Same flow and isolation as tests/semantic_audit.rs: copy the fixture's
//! `src` into an isolated repo dir, `init --no-hooks --no-map`, `map src`,
//! then `audit --semantic`, every command through the COMPILED BINARY with
//! HOME/SCOUT_REGISTRY/SCOUT_CONTENT_DB pointed at a fresh temp dir.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-semantic-direction-{prefix}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-direction")
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

        copy_tree_skip_build_output(&fixture_root().join("src"), &repo.join("src"));

        let fx = Fixture { repo, home };
        let init = fx.run(&["init", "--no-hooks", "--no-map"]);
        assert!(init.status.success(), "init failed: {init:?}");
        let map = fx.run(&["map", "src"]);
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
fn audit_scores_the_direction_fixture_against_the_committed_oracle_snapshot() {
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

    // 60 precise edges. The four false positives are the documented shapes
    // the resolver cannot decide from names and arity alone, each pinned by
    // its own `// case` comment in the fixture:
    //   - `this.Stamp()` inside a class that derives from a base declaring
    //     Stamp AND explicitly implements an interface's Stamp (the explicit
    //     implementation sits in the class's non-public member list under
    //     its bare name);
    //   - an `internal new` member hiding a public base member (only
    //     `public` counts as visible to a receiver other than `this`);
    //   - a `private new` field shadowing a public base property, read from
    //     outside the type (fields and properties carry every
    //     accessibility);
    //   - same-arity overloads split across base and derived by parameter
    //     TYPE (the arity gate cannot see types).
    // Everything else the compiler decides along an `inherits` edge --
    // inherited, overridden and hidden members through derived, base-typed,
    // `this.` and `base.` receivers, interface members through interface,
    // implementing-class and base-interface receivers, static members
    // through bare, qualified and generic derived type names -- scores as a
    // true positive.
    assert_eq!(v["tiers"]["precise"]["edges"], 60, "{stdout}");
    assert_eq!(v["tiers"]["precise"]["tp"], 56, "{stdout}");
    assert_eq!(v["tiers"]["precise"]["fp"], 4, "{stdout}");
    assert!(
        v["tiers"]["ext"].is_null() && v["tiers"]["guess"].is_null(),
        "the fixture has no extension methods and every receiver resolves in-graph, so no \
         heuristic tier emits: {stdout}"
    );
    // Six oracle records earn no edge at all, every one outside this
    // fixture's question: four bare unqualified calls (unrecorded, and
    // excluded from recall's denominator, which counts `access` records
    // only), one cast receiver `((IContract)x).Fulfil()` (untyped), and one
    // `internal override` (invisible to a non-`this` receiver). Recall's own
    // six misses over its 62 sites are the two no-edge access records plus
    // the four false-positive sites above, whose edge names the wrong type.
    let recall_all = v["recall"]["all"].as_f64().expect("recall.all is a number");
    assert!(recall_all >= 0.9, "recall.all = {recall_all}: {stdout}");
    assert_eq!(v["structural"]["impossible"], 0, "{stdout}");
    assert_eq!(v["units"]["failed"], 0, "{stdout}");
}

#[test]
fn audit_assert_passes_the_direction_fixture_thresholds() {
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
    let stdout = stdout_of(&passing);
    assert!(
        stdout.contains("devscout audit --semantic"),
        "the report is printed on stdout even with --assert: {stdout}"
    );
}
