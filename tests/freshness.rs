// Index freshness. `devscout map` stamps a repo-relative sidecar
// (`index-state.json`, beside `manifest.json` under `.git/scout/`) with the
// HEAD it ran against; `find`/`refs`/`impact` compare that against the live
// HEAD and working tree on every call and warn on stderr, exactly once, when
// they disagree.
//
// Every test here goes through the COMPILED BINARY as a subprocess -- same
// reason tests/cli_zero_hit.rs does: stdout has to stay byte-identical to what
// it printed before the warning existed, and the freshness line's own
// presence/absence is half of what is under test, so an in-process `cmd_*`
// call could not tell the two apart.
//
// SAFETY: every spawned process gets HOME, SCOUT_REGISTRY and SCOUT_CONTENT_DB
// pointed at a fresh temp dir, same as cli_zero_hit.rs, so the operator's
// real ~/.claude/settings.json and repos.json are never read and never
// written.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!("scout-freshness-{prefix}-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn rust_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_devscout"))
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(dir).stdin(Stdio::null()).output().expect("git binary must be on PATH");
    assert!(output.status.success(), "git {args:?} failed in {}: {output:?}", dir.display());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

// `WidgetImpl.cs` gives `impact`/`refs` an actual (non-zero-hit) inbound edge
// to reach, isolating the freshness assertions from the unrelated zero-hit
// stderr note.
const FILES: &[(&str, &str)] = &[
    ("src/IWidget.cs", "namespace App.Widgets\n{\n    public interface IWidget\n    {\n        void Render();\n    }\n}\n"),
    (
        "src/WidgetImpl.cs",
        "using App.Widgets;\n\nnamespace App.Widgets.Impl\n{\n    public class WidgetImpl : IWidget\n    {\n        public void Render() { }\n    }\n}\n",
    ),
];

struct Fixture {
    root: PathBuf,
    home: PathBuf,
}

impl Fixture {
    // Source files are committed here (via `add` + `write-tree` +
    // `commit-tree` -- never `git commit`, matching repository policy even
    // for a throwaway fixture) rather than left untracked the way
    // cli_zero_hit.rs's fixture is: an untracked-but-mapped file always
    // differs from HEAD, forever, regardless of whether its content has
    // actually changed since the last `devscout map`, which would make the
    // "fresh" case indistinguishable from the "changed" one below.
    //
    // Returns the fixture and the sha of this initial commit (what
    // `index-state.json` must have recorded as `head`).
    fn build(prefix: &str) -> (Fixture, String) {
        let base = temp_dir(prefix);
        let root = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&home).expect("create home dir");
        for (rel, body) in FILES {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).expect("create fixture dir");
            fs::write(&path, body).expect("write fixture file");
        }
        git(&root, &["init", "-q", "."]);
        git(&root, &["add", "-A"]);
        let tree = git(&root, &["write-tree"]);
        let sha = git(&root, &["commit-tree", &tree, "-m", "init"]);
        git(&root, &["update-ref", "refs/heads/master", &sha]);
        git(&root, &["symbolic-ref", "HEAD", "refs/heads/master"]);

        let fixture = Fixture { root, home };
        let init = fixture.run(&["init", "--no-hooks", "--no-map"]);
        assert!(init.status.success(), "init failed: {init:?}");
        let map = fixture.run(&["map", "."]);
        assert!(map.status.success(), "map failed: {map:?}");
        (fixture, sha)
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

    // Advances HEAD to a new commit over the SAME tree (never `git commit`)
    // -- working tree and index are untouched, only the ref moves, isolating
    // "HEAD moved" from "a file changed". Reusing an empty tree here instead
    // would make the new commit's tree disagree with the (non-empty) index,
    // which `git status` reports as staged changes -- exactly the "a file
    // changed" signal this must NOT also trigger.
    fn advance_head(&self, parent_sha: &str) -> String {
        let tree = git(&self.root, &["rev-parse", &format!("{parent_sha}^{{tree}}")]);
        let sha = git(&self.root, &["commit-tree", &tree, "-p", parent_sha, "-m", "advance"]);
        git(&self.root, &["update-ref", "refs/heads/master", &sha]);
        sha
    }
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("stderr is utf-8")
}

#[test]
fn fresh_index_prints_no_freshness_warning_on_any_of_the_three_verbs() {
    let (fx, _sha) = Fixture::build("fresh");

    let find = fx.run(&["find", "IWidget"]);
    assert_eq!(stderr_of(&find), "", "{find:?}");

    let refs = fx.run(&["refs", "IWidget"]);
    assert_eq!(stderr_of(&refs), "", "{refs:?}");

    let impact = fx.run(&["impact", "src/IWidget.cs"]);
    assert_eq!(stderr_of(&impact), "", "{impact:?}");
    assert_eq!(impact.status.code(), Some(0), "WidgetImpl.cs gives impact a real inbound edge to reach: {impact:?}");
}

#[test]
fn head_moved_since_map_prints_exactly_one_stale_index_line() {
    let (fx, initial_sha) = Fixture::build("head-moved");
    let new_sha = fx.advance_head(&initial_sha);
    assert_ne!(new_sha, initial_sha);

    let refs = fx.run(&["refs", "IWidget"]);
    let want = format!(
        "devscout: index for repo is stale (indexed at {}, HEAD {}; 0 changed files) — rebuild with devscout map\n",
        &initial_sha[..7],
        &new_sha[..7],
    );
    assert_eq!(stderr_of(&refs), want);
}

#[test]
fn a_modified_indexed_file_head_unchanged_is_reported_as_one_changed_file() {
    let (fx, _sha) = Fixture::build("modified-file");
    fs::write(
        fx.root.join("src").join("IWidget.cs"),
        "namespace App.Widgets\n{\n    public interface IWidget\n    {\n        void Render();\n        void Resize();\n    }\n}\n",
    )
    .unwrap();

    let find = fx.run(&["find", "IWidget"]);
    let stderr = stderr_of(&find);
    assert!(stderr.starts_with("devscout: index for repo is stale (indexed at "), "{stderr:?}");
    assert!(stderr.contains("; 1 changed files) — rebuild with devscout map\n"), "{stderr:?}");
}

#[test]
fn legacy_index_with_no_index_state_json_prints_no_warning_and_does_not_crash() {
    let (fx, _sha) = Fixture::build("legacy");
    // Simulate an index built before the sidecar existed.
    let state_path = fx.root.join(".git").join("scout").join("index-state.json");
    assert!(state_path.exists(), "map must have written the sidecar: {state_path:?}");
    fs::remove_file(&state_path).unwrap();

    let find = fx.run(&["find", "IWidget"]);
    assert_eq!(find.status.code(), Some(0), "{find:?}");
    assert_eq!(stderr_of(&find), "");

    let refs = fx.run(&["refs", "IWidget"]);
    assert_eq!(stderr_of(&refs), "");
}
