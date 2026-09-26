//! Shared harness for integration tests that admit a hand-built
//! compiler-facts artifact into a real git copy of
//! `fixtures/csharp-enrichment` and run the compiled binary against it.
//!
//! Every candidate is authored field by field here, never taken from a
//! producer run, so an expectation can never merely echo the output it
//! judges. See `fixtures/csharp-enrichment/README.md`.

#![allow(dead_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use devscout_rs::graph;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub const CALLER_FILE: &str = "src/Caller.cs";
pub const COMPILATION_FINGERPRINT: &str = "1111111111111111111111111111111111111111";

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!("scout-lane-{prefix}-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-enrichment")
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create dest dir");
    for entry in fs::read_dir(src).expect("read source dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name();
        let file_type = entry.file_type().expect("entry file type");
        if file_type.is_dir() {
            copy_tree(&entry.path(), &dst.join(&name));
        } else if file_type.is_file() {
            fs::copy(entry.path(), dst.join(&name)).expect("copy fixture file");
        }
    }
}

// Mirrors the crate-private context summary fold so a hand-built candidate
// passes admission's context-fingerprint check.
fn context_fingerprint(identity: &Value, fingerprint: &str) -> String {
    let canonical = serde_json::to_string(identity).expect("identity serializes");
    let digest = Sha256::digest(format!("{canonical}\u{1f}{fingerprint}").as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Lower-case hex SHA-1, the encoding the producer records for an import.
pub fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::Digest as _;
    sha1::Sha1::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub struct Fixture {
    pub repo: PathBuf,
    pub home: PathBuf,
}

impl Fixture {
    /// An isolated one-commit git copy of the fixture tree, registered with
    /// `init --no-hooks --no-map`. `home` is the repository's sibling, so an
    /// import identity of `external-file:../home/<name>` names a file there.
    pub fn build(prefix: &str) -> Fixture {
        let base = temp_dir(prefix);
        let repo = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&repo).expect("create repo dir");
        fs::create_dir_all(&home).expect("create home dir");
        copy_tree(&fixture_root().join("src"), &repo.join("src"));

        let fx = Fixture { repo, home };
        fx.git(&["init", "-q"]);
        fx.git(&["config", "user.email", "test@example.com"]);
        fx.git(&["config", "user.name", "test"]);
        fx.git(&["add", "."]);
        fx.git(&["commit", "-q", "-m", "fixture commit"]);
        let init = fx.run(&["init", "--no-hooks", "--no-map"]);
        assert!(init.status.success(), "init failed: {init:?}");
        fx
    }

    pub fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .args(args)
            .current_dir(&self.repo)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .output()
            .expect("devscout must run")
    }

    pub fn git(&self, args: &[&str]) -> Output {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.repo)
            .output()
            .expect("git must run");
        assert!(out.status.success(), "git {args:?} failed: {out:?}");
        out
    }

    pub fn head(&self) -> String {
        let out = self.git(&["rev-parse", "HEAD"]);
        String::from_utf8(out.stdout)
            .expect("git output is utf-8")
            .trim()
            .to_string()
    }

    /// Commits a file outside the mapped `src` scope, moving HEAD without
    /// touching any mapped source.
    pub fn commit_unrelated_file(&self) {
        fs::write(self.repo.join("notes.txt"), "notes\n").expect("write notes");
        self.git(&["add", "notes.txt"]);
        self.git(&["commit", "-q", "-m", "notes"]);
    }

    /// `map src` with any extra flags; asserts exit 0 and returns stdout.
    pub fn map(&self, extra: &[&str]) -> String {
        let mut args = vec!["map", "src"];
        args.extend_from_slice(extra);
        let out = self.run(&args);
        assert!(out.status.success(), "map failed: {out:?}");
        String::from_utf8(out.stdout).expect("stdout is utf-8")
    }

    pub fn graph_bytes(&self) -> Vec<u8> {
        let path = graph::graph_json_path(&self.repo);
        fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    pub fn graph(&self) -> Value {
        serde_json::from_slice(&self.graph_bytes()).expect("graph.json is valid JSON")
    }

    pub fn artifact_path(&self) -> PathBuf {
        graph::compiler_facts_json_path(&self.repo)
    }

    /// The first ambiguous `Config.Load()` call's line, read back from a
    /// syntax-only graph rather than hand-counted.
    pub fn first_load_line(&self) -> u64 {
        let g = self.graph();
        g["edges"]
            .as_array()
            .expect("edges array")
            .iter()
            .filter(|e| {
                e["kind"] == "uses-member" && e["from_file"] == CALLER_FILE && e["member"] == "Load"
            })
            .filter_map(|e| e["from_line"].as_u64())
            .min()
            .expect("the fixture has an ambiguous Config.Load() call")
    }

    /// Admits `candidate` through `compiler-facts import`, written outside the
    /// repository so the candidate file never dirties the tree.
    pub fn import(&self, candidate: &Value) {
        let path = self.home.join(format!(
            "candidate-{}.json",
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        fs::write(&path, serde_json::to_vec(candidate).expect("serializes"))
            .expect("write candidate");
        let out = self.run(&[
            "compiler-facts",
            "import",
            path.to_str().expect("utf-8 path"),
        ]);
        assert!(out.status.success(), "compiler-facts import: {out:?}");
    }

    /// `audit --semantic` over an empty oracle, returning the reported lane.
    pub fn audit_lane(&self) -> String {
        let refs = self.home.join("empty-refs.jsonl");
        fs::write(&refs, "").expect("write empty refs");
        let out = self.run(&[
            "audit",
            "--semantic",
            refs.to_str().expect("utf-8 path"),
            "--json",
        ]);
        assert!(out.status.success(), "audit failed: {out:?}");
        let report: Value = serde_json::from_slice(&out.stdout).expect("audit --json is JSON");
        report["lane"]
            .as_str()
            .expect("lane is a string")
            .to_string()
    }
}

pub fn compilation_identity() -> Value {
    json!({
        "projectPath": "Fixture.csproj",
        "projectName": "Fixture",
        "requestedTfm": "net9.0",
        "effectiveTfm": "net9.0",
        "configuration": "Debug",
        "platform": "AnyCPU",
    })
}

/// A confirmed `Beta.Config.Load` fact at `line`, which displaces one of the
/// two guessed edges the syntax ladder pushes there.
pub fn confirmed_load_site(line: u64) -> Value {
    json!({
        "file": CALLER_FILE,
        "shape": "call",
        "span": {"startLine": line, "startChar": 0, "endLine": line, "endChar": 1},
        "name": {"line": line, "char": 0},
        "caller": {
            "assembly": "Fixture",
            "type": "Fixtures.Enrichment.Caller",
            "member": "unspecified",
            "genericArity": 0,
            "overloadSignature": "()->void",
        },
        "resolution": "confirmed",
        "candidateReason": "",
        "target": {
            "assembly": "Fixture",
            "type": "Fixtures.Enrichment.Beta.Config",
            "member": "Load",
            "genericArity": 0,
            "overloadSignature": "()->void",
        },
        "candidates": [],
        "compilation": {"identity": compilation_identity(), "fingerprint": COMPILATION_FINGERPRINT},
        "documentContentIdentity": "doc-caller",
        "targetDocumentContentIdentities": [],
    })
}

/// A complete, admissible candidate captured at `head_sha`, with one
/// compilation carrying `imports` and the given occurrence sites.
pub fn candidate(head_sha: &str, imports: Vec<Value>, sites: Vec<Value>) -> Value {
    let identity = compilation_identity();
    json!({
        "format": graph::COMPILER_FACTS_FORMAT,
        "contractVersion": graph::COMPILER_FACTS_CONTRACT_VERSION,
        "artifactSchemaVersion": graph::COMPILER_FACTS_ARTIFACT_SCHEMA_VERSION,
        "producer": {"name": graph::EXPECTED_PRODUCER_NAME, "engineRevision": graph::EXPECTED_ENGINE_REVISION},
        "profile": {"target": "net9.0", "configuration": "Debug", "platform": "AnyCPU"},
        "dependencyFingerprint": graph::EXPECTED_DEPENDENCY_FINGERPRINT,
        "context": {
            "schemaVersion": graph::EXPECTED_CONTEXT_SCHEMA_VERSION,
            "contextFingerprint": context_fingerprint(&identity, COMPILATION_FINGERPRINT),
            "envelope": {
                "compilations": [{
                    "identity": identity,
                    "state": "complete",
                    "reason": "",
                    "fingerprint": COMPILATION_FINGERPRINT,
                    "imports": imports,
                }],
            },
        },
        "sourceSnapshot": {"headSha": head_sha, "dirty": false, "dirtyDigest": ""},
        "capabilities": {"requested": ["occurrences"], "provided": ["occurrences"]},
        "completion": {"terminal": true},
        "units": {"processed": ["Fixture.csproj"], "missing": []},
        "coverage": {"state": "complete"},
        "diagnostics": [],
        "symbols": [],
        "occurrences": {
            "spanEncoding": "utf16-code-unit-line1-char0-end-exclusive",
            "identityEncoding": "fully-qualified-display-format",
            "sites": sites,
        },
    })
}

/// The second line of `map`'s stdout, if any.
pub fn lane_line(stdout: &str) -> Option<&str> {
    stdout.lines().nth(1)
}
