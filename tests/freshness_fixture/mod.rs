//! Shared fixture for the index-freshness integration tests.
//!
//! Every test drives the COMPILED BINARY as a subprocess in a throwaway git
//! repository: freshness is a property of what one invocation decides, and the
//! stderr lines and exit codes are half of what is under test, so an
//! in-process call could not stand in for it.
//!
//! SAFETY: each spawned process gets `HOME`, `SCOUT_REGISTRY` and
//! `SCOUT_CONTENT_DB` pointed at a fresh temp directory, so the operator's real
//! `~/.claude/settings.json` and `repos.json` are never read or written, and no
//! fixture shares a content store with another.

#![allow(dead_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// The C# pair every fixture starts from: a type and one implementation of it,
/// so `refs`/`impact`/`tests` have a real inbound edge and their answers are
/// distinguishable from a zero-hit miss.
pub const BASELINE: &[(&str, &str)] = &[
    (
        "src/IWidget.cs",
        "namespace App.Widgets\n{\n    public interface IWidget\n    {\n        void Render();\n    }\n}\n",
    ),
    (
        "src/WidgetImpl.cs",
        "using App.Widgets;\n\nnamespace App.Widgets.Impl\n{\n    public class WidgetImpl : IWidget\n    {\n        public void Render() { }\n    }\n}\n",
    ),
];

pub fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let wanted = env::temp_dir().join(format!(
        "scout-freshness-it-{prefix}-{}-{n}",
        std::process::id()
    ));
    fs::canonicalize(fresh_dir(wanted)).expect("canonicalize temp dir")
}

/// Claims a new directory at `wanted`, or at the first free `-<n>` variant.
///
/// A name is never reopened, because a run whose process id and counter repeat
/// an earlier run's would otherwise reopen that run's directory: its fixtures,
/// its `git add -A`, and the files this test writes would then agree, so a query
/// would answer `fresh` where the test expects a budget answer.
pub fn fresh_dir(wanted: PathBuf) -> PathBuf {
    let name = wanted
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    for attempt in 0..10_000u32 {
        let candidate = if attempt == 0 {
            wanted.clone()
        } else {
            wanted.with_file_name(format!("{name}-{attempt}"))
        };
        match fs::create_dir(&candidate) {
            Ok(()) => return candidate,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => panic!("create fixture dir {}: {e}", candidate.display()),
        }
    }
    panic!("no free fixture directory name for {}", wanted.display());
}

pub fn rust_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_devscout"))
}

/// Runs `git` in `dir` and returns trimmed stdout, asserting success.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("GIT_AUTHOR_NAME", "devscout-test")
        .env("GIT_AUTHOR_EMAIL", "devscout-test@example.com")
        .env("GIT_COMMITTER_NAME", "devscout-test")
        .env("GIT_COMMITTER_EMAIL", "devscout-test@example.com")
        .output()
        .expect("git binary must be on PATH");
    assert!(
        output.status.success(),
        "git {args:?} failed in {}: {output:?}",
        dir.display()
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

pub struct Fixture {
    pub base: PathBuf,
    pub root: PathBuf,
    pub home: PathBuf,
}

impl Fixture {
    /// A git repository holding `files`, committed as one initial commit, then
    /// registered and mapped. Returns the fixture and the commit's sha.
    pub fn mapped(prefix: &str, files: &[(&str, &str)]) -> (Fixture, String) {
        let fixture = Fixture::empty(prefix);
        for (rel, body) in files {
            fixture.write(rel, body);
        }
        let sha = fixture.commit_all("init");
        let init = fixture.run(&["init", "--no-hooks", "--no-map"]);
        assert!(init.status.success(), "init failed: {init:?}");
        let map = fixture.run(&["map", "."]);
        assert!(map.status.success(), "map failed: {map:?}");
        (fixture, sha)
    }

    /// An empty git repository with no commit yet.
    pub fn empty(prefix: &str) -> Fixture {
        let base = temp_dir(prefix);
        let root = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&root).expect("create repo dir");
        fs::create_dir_all(&home).expect("create home dir");
        git(&root, &["init", "-q", "-b", "main", "."]);
        Fixture { base, root, home }
    }

    pub fn write(&self, rel: &str, body: &str) {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture dir");
        }
        fs::write(&path, body).expect("write fixture file");
    }

    pub fn remove(&self, rel: &str) {
        fs::remove_file(self.root.join(rel)).expect("remove fixture file");
    }

    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.root.join(rel)).expect("read fixture file")
    }

    /// Commits the working tree without `git commit`, so no author identity and
    /// no commit-message policy is involved.
    pub fn commit_all(&self, message: &str) -> String {
        git(&self.root, &["add", "-A"]);
        let tree = git(&self.root, &["write-tree"]);
        let parent = self.head();
        let sha = if parent.is_empty() {
            git(&self.root, &["commit-tree", &tree, "-m", message])
        } else {
            git(
                &self.root,
                &["commit-tree", &tree, "-p", &parent, "-m", message],
            )
        };
        git(&self.root, &["update-ref", "refs/heads/main", &sha]);
        sha
    }

    /// Advances HEAD to a new commit over the SAME tree, so "HEAD moved" is
    /// isolated from "a file changed".
    pub fn advance_head(&self) -> String {
        let parent = self.head();
        let tree = git(&self.root, &["rev-parse", &format!("{parent}^{{tree}}")]);
        let sha = git(
            &self.root,
            &["commit-tree", &tree, "-p", &parent, "-m", "advance"],
        );
        git(&self.root, &["update-ref", "refs/heads/main", &sha]);
        sha
    }

    pub fn head(&self) -> String {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&self.root)
            .output()
            .expect("git must run");
        if !out.status.success() {
            return String::new();
        }
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    /// A linked worktree at the current HEAD, detached so both worktrees share
    /// one commit.
    pub fn add_worktree(&self, name: &str) -> PathBuf {
        let path = self.base.join(name);
        git(
            &self.root,
            &[
                "worktree",
                "add",
                "--detach",
                path.to_str().unwrap(),
                "HEAD",
            ],
        );
        fs::canonicalize(&path).expect("canonicalize linked worktree")
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.run_in(&self.root, args)
    }

    pub fn run_in(&self, dir: &Path, args: &[&str]) -> Output {
        Command::new(rust_bin())
            .args(args)
            .current_dir(dir)
            .env("HOME", &self.home)
            // Pinned too: with `HOME` moved but `XDG_CONFIG_HOME` inherited, Git
            // would still read the developer's own global config and ignore
            // file, so a rule the fixture never wrote could decide the run.
            .env("XDG_CONFIG_HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .output()
            .expect("devscout must run")
    }

    /// The record file's path, beside the manifest under the shared scout dir.
    pub fn record_path(&self) -> PathBuf {
        self.root
            .join(".git")
            .join("scout")
            .join("index-state.json")
    }

    pub fn digests_path(&self) -> PathBuf {
        self.root
            .join(".git")
            .join("scout")
            .join("index-digests.json")
    }

    /// Writes a record in the shape an earlier release produced: a top-level
    /// `head` and no owner, generation or digests.
    pub fn write_legacy_record(&self, head: &str) {
        let path = self.record_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            format!(
                "{{\n  \"head\": \"{head}\",\n  \"dirty\": false,\n  \"indexed_at\": \"2020-01-01T00:00:00Z\",\n  \"file_count\": 2,\n  \"dirty_indexed_files\": []\n}}\n"
            ),
        )
        .unwrap();
    }

    /// Replaces the record with the in-progress marker a `map` writes first.
    pub fn write_marker(&self, owner: &str) {
        let path = self.record_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            format!(
                "{{\n  \"format\": \"devscout-index-state\",\n  \"version\": 2,\n  \"state\": \"in-progress\",\n  \"owner\": \"{owner}\",\n  \"started_at\": \"2020-01-01T00:00:00Z\"\n}}\n"
            ),
        )
        .unwrap();
    }

    pub fn record(&self) -> Value {
        serde_json::from_str(&self.read_abs(&self.record_path())).expect("record is JSON")
    }

    pub fn read_abs(&self, path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    /// The record's `owner`.
    pub fn owner(&self) -> String {
        self.record()["owner"].as_str().unwrap().to_string()
    }

    /// The graph artifact, for the tests that rewrite it in place.
    pub fn graph_path(&self) -> PathBuf {
        self.root
            .join(".git")
            .join("scout")
            .join("graph")
            .join("graph.json")
    }
}

pub fn stderr_of(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("stderr is utf-8")
}

pub fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

/// The `freshness` object of a `--json` answer.
pub fn freshness_of(out: &Output) -> Value {
    let text = stdout_of(out);
    let value: Value = serde_json::from_str(text.trim()).unwrap_or_else(|e| {
        panic!("answer is not JSON ({e}): {text}");
    });
    value
        .get("freshness")
        .cloned()
        .unwrap_or_else(|| panic!("no freshness key in {text}"))
}

pub fn state_of(out: &Output) -> String {
    freshness_of(out)["state"].as_str().unwrap().to_string()
}
