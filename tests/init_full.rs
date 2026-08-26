//! Integration tests for full repository initialization.

// End-to-end coverage of the out-of-the-box steps `devscout init` layers on
// top of registering the repo -- language census, hook install, first map.
// This file drives `cmd_init_full` (initcmd.rs, wired in cli.rs) through the
// binary; the steps below add unconditional stdout content, which is why they
// are pinned here rather than folded into initcmd.rs's own stdout assertions.
//
// SAFETY (HARD RULE, non-negotiable): every single process this file spawns
// gets HOME pointed at an isolated fresh temp dir via `Command::env`, which
// overrides the child process's HOME regardless of this test binary's own
// real HOME -- the live `~/.claude/settings.json` is never read, and never
// written, by anything in this file. Every test here goes through the COMPILED
// BINARY as a subprocess, so no process-global env var is mutated anywhere in
// this file: there is nothing to race and no lock is needed.
// `initcmd::cmd_init_full` is never called in-process here on purpose,
// precisely because it is the one entry point that touches
// `$HOME/.claude/settings.json`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-init-full-{prefix}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    // macOS's `env::temp_dir()` is itself a symlink (`/var` ->
    // `/private/var`), and `git` internally realpaths directories it manages
    // -- canonicalizing once here keeps every path this file constructs
    // already in the form git (and this crate's non-symlink-following path
    // helpers) will agree on.
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

/// One isolated env triple per test -- registry + content-db + HOME, all
/// under one fresh temp dir. `home` is what every subprocess below gets as
/// its `HOME`; `$HOME/.claude/settings.json` under it is a purely synthetic
/// file this test controls end to end, never the operator's real one.
struct Env {
    registry: PathBuf,
    content_db: PathBuf,
    home: PathBuf,
}

fn fresh_env(prefix: &str) -> Env {
    let base = temp_dir(&format!("{prefix}-env"));
    let home = base.join("home");
    fs::create_dir_all(&home).unwrap();
    Env {
        registry: base.join("repos.json"),
        content_db: base.join("content.db"),
        home,
    }
}

fn settings_path(env_vars: &Env) -> PathBuf {
    env_vars.home.join(".claude").join("settings.json")
}

fn seed_settings(env_vars: &Env, body: &str) {
    let dir = env_vars.home.join(".claude");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("settings.json"), body).unwrap();
}

fn backup_files(env_vars: &Env) -> Vec<PathBuf> {
    let dir = env_vars.home.join(".claude");
    fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("settings.json.bak.")
                })
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

fn run_init(cwd: &Path, args: &[&str], env_vars: &Env) -> (String, String, i32) {
    let output = Command::new(rust_bin())
        .arg("init")
        .args(args)
        .current_dir(cwd)
        .env("SCOUT_REGISTRY", &env_vars.registry)
        .env("SCOUT_CONTENT_DB", &env_vars.content_db)
        .env("HOME", &env_vars.home)
        .output()
        .expect("devscout binary must be built (run `cargo build` first)");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

fn run_find(cwd: &Path, query: &str, env_vars: &Env) -> (String, i32) {
    let output = Command::new(rust_bin())
        .args(["find", query])
        .current_dir(cwd)
        .env("SCOUT_REGISTRY", &env_vars.registry)
        .env("SCOUT_CONTENT_DB", &env_vars.content_db)
        .env("HOME", &env_vars.home)
        .output()
        .expect("devscout binary must be built");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

/// A synthetic git repo with one AST-supported (`.cs`) and one
/// present-but-unsupported (`.md`) source file -- enough for the census,
/// the first map, and a `find` hit, all in one fixture.
fn init_git_repo_with_source(prefix: &str) -> PathBuf {
    let root = temp_dir(prefix);
    run_git(&root, &["init", "-q"]);
    bootstrap_initial_commit(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/Widget.cs"),
        "namespace Fixtures.InitFull\n{\n    public class Widget\n    {\n        public void Render() {}\n    }\n}\n",
    )
    .unwrap();
    fs::write(root.join("src/notes.md"), "# notes\nplain text\n").unwrap();
    root
}

/// Mirrors the real ~/.claude/settings.json shape (top-level `model`,
/// `hooks.PreToolUse`, `hooks.PostToolUse` with pre-existing, unrelated hook
/// entries that do NOT contain the literal text "hook read"/"hook bash" -- so
/// the idempotency check must NOT mistake them for already-installed).
const WELL_FORMED_SETTINGS: &str = r#"{
  "model": "test-model",
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [ { "type": "command", "command": "some-other-tool" } ] }
    ],
    "PostToolUse": [
      { "matcher": "Read", "hooks": [ { "type": "command", "command": "node /somewhere/scout-read-hook.js" } ] },
      { "matcher": "Bash", "hooks": [ { "type": "command", "command": "node /somewhere/scout-bash-hook.js" } ] }
    ]
  }
}
"#;

// ---------------------------------------------------------------------------
// Hook install: adds both entries, correct shape, backup created.
// ---------------------------------------------------------------------------

#[test]
fn hooks_install_adds_both_entries_with_correct_shape_and_creates_backup() {
    let root = init_git_repo_with_source("hooks-install");
    let env_vars = fresh_env("hooks-install");
    seed_settings(&env_vars, WELL_FORMED_SETTINGS);
    let path = settings_path(&env_vars);
    let original_bytes = fs::read(&path).unwrap();

    let (out, err, code) = run_init(&root, &[], &env_vars);
    assert_eq!(code, 0, "stdout: {out}\nstderr: {err}");
    assert!(out.contains("hooks: installed"), "got: {out}");

    let updated: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let ptu = updated["hooks"]["PostToolUse"]
        .as_array()
        .expect("PostToolUse must be an array");
    assert_eq!(
        ptu.len(),
        4,
        "2 pre-existing (unrelated) + 2 new (devscout hook read/bash)"
    );

    let commands: Vec<&str> = ptu
        .iter()
        .flat_map(|e| e["hooks"].as_array().unwrap())
        .map(|h| h["command"].as_str().unwrap())
        .collect();
    assert!(
        commands
            .iter()
            .any(|c| c.ends_with(" hook read") && c.starts_with('/')),
        "commands: {commands:?}"
    );
    assert!(
        commands
            .iter()
            .any(|c| c.ends_with(" hook bash") && c.starts_with('/')),
        "commands: {commands:?}"
    );
    // The pre-existing, unrelated entries must survive untouched.
    assert!(commands.contains(&"node /somewhere/scout-read-hook.js"));
    assert!(commands.contains(&"node /somewhere/scout-bash-hook.js"));

    // Untouched sibling keys, preserved in place.
    assert_eq!(updated["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    assert_eq!(updated["model"], "test-model");

    // New matcher entries carry `"type": "command"`.
    for entry in ptu {
        for h in entry["hooks"].as_array().unwrap() {
            assert_eq!(h["type"], "command");
        }
    }

    let backups = backup_files(&env_vars);
    assert_eq!(backups.len(), 1, "exactly one backup created");
    assert_eq!(
        fs::read(&backups[0]).unwrap(),
        original_bytes,
        "backup preserves the ORIGINAL bytes verbatim, pre-modification"
    );
}

// ---------------------------------------------------------------------------
// Idempotency: re-run adds nothing, settings byte-identical, no 2nd backup.
// ---------------------------------------------------------------------------

#[test]
fn hooks_install_is_idempotent_on_rerun() {
    let root = init_git_repo_with_source("hooks-idempotent");
    let env_vars = fresh_env("hooks-idempotent");
    seed_settings(&env_vars, WELL_FORMED_SETTINGS);
    let path = settings_path(&env_vars);

    let (out1, err1, code1) = run_init(&root, &[], &env_vars);
    assert_eq!(code1, 0, "stdout: {out1}\nstderr: {err1}");
    assert!(out1.contains("hooks: installed"), "got: {out1}");
    let after_first = fs::read_to_string(&path).unwrap();

    let (out2, err2, code2) = run_init(&root, &[], &env_vars);
    assert_eq!(code2, 0, "stdout: {out2}\nstderr: {err2}");
    assert!(out2.contains("hooks: already installed"), "got: {out2}");
    let after_second = fs::read_to_string(&path).unwrap();

    assert_eq!(
        after_first, after_second,
        "re-run must leave settings.json byte-identical"
    );
    assert_eq!(
        backup_files(&env_vars).len(),
        1,
        "idempotent re-run must not create a second backup"
    );
}

// ---------------------------------------------------------------------------
// --no-hooks: settings untouched, no backup.
// ---------------------------------------------------------------------------

#[test]
fn no_hooks_flag_leaves_settings_byte_untouched() {
    let root = init_git_repo_with_source("no-hooks-flag");
    let env_vars = fresh_env("no-hooks-flag");
    seed_settings(&env_vars, WELL_FORMED_SETTINGS);
    let path = settings_path(&env_vars);
    let before = fs::read(&path).unwrap();

    let (out, err, code) = run_init(&root, &["--no-hooks"], &env_vars);
    assert_eq!(code, 0, "stdout: {out}\nstderr: {err}");
    assert!(out.contains("hooks: skipped (--no-hooks)"), "got: {out}");

    let after = fs::read(&path).unwrap();
    assert_eq!(
        before, after,
        "settings.json must be byte-identical when --no-hooks is passed"
    );
    assert!(
        backup_files(&env_vars).is_empty(),
        "no backup when the hooks step never runs"
    );
}

// ---------------------------------------------------------------------------
// Unexpected shape: left untouched, snippet printed, exit still 0 (core
// parity steps succeeded; only the hooks step declined).
// ---------------------------------------------------------------------------

#[test]
fn unexpected_shape_settings_left_untouched_and_snippet_printed() {
    let root = init_git_repo_with_source("bad-shape");
    let env_vars = fresh_env("bad-shape");
    // "hooks" present but not an object -- an unrecognized shape.
    seed_settings(&env_vars, r#"{"hooks": "not-an-object"}"#);
    let path = settings_path(&env_vars);
    let before = fs::read(&path).unwrap();

    let (out, err, code) = run_init(&root, &[], &env_vars);
    assert_eq!(code, 0, "core init must still succeed even though the hooks step declines; stdout: {out}\nstderr: {err}");
    assert!(out.contains("unexpected shape"), "got: {out}");
    assert!(
        out.contains("\"PostToolUse\""),
        "the exact JSON snippet must be printed inline; got: {out}"
    );
    assert!(
        out.contains("hook read") && out.contains("hook bash"),
        "got: {out}"
    );

    let after = fs::read(&path).unwrap();
    assert_eq!(
        before, after,
        "unexpected-shape settings file must be left byte-untouched"
    );
    assert!(
        backup_files(&env_vars).is_empty(),
        "no write attempted -- no backup either"
    );
}

#[test]
fn invalid_json_settings_left_untouched_and_snippet_printed() {
    let root = init_git_repo_with_source("invalid-json");
    let env_vars = fresh_env("invalid-json");
    seed_settings(&env_vars, "{ not valid json");
    let path = settings_path(&env_vars);
    let before = fs::read(&path).unwrap();

    let (out, err, code) = run_init(&root, &[], &env_vars);
    assert_eq!(code, 0, "stdout: {out}\nstderr: {err}");
    assert!(out.contains("not valid JSON"), "got: {out}");
    assert!(out.contains("\"PostToolUse\""), "got: {out}");

    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "invalid-JSON settings file must be left byte-untouched"
    );
    assert!(backup_files(&env_vars).is_empty());
}

#[test]
fn missing_settings_file_prints_snippet_and_writes_nothing() {
    let root = init_git_repo_with_source("missing-settings");
    let env_vars = fresh_env("missing-settings");
    let path = settings_path(&env_vars);
    assert!(
        !path.exists(),
        "fixture precondition: no .claude dir at all under this HOME"
    );

    let (out, err, code) = run_init(&root, &[], &env_vars);
    assert_eq!(code, 0, "stdout: {out}\nstderr: {err}");
    assert!(out.contains("no settings file"), "got: {out}");
    assert!(out.contains("\"PostToolUse\""), "got: {out}");
    assert!(
        out.contains("hook read") && out.contains("hook bash"),
        "got: {out}"
    );

    assert!(
        !path.exists(),
        "must not create a settings.json out of thin air"
    );
}

// ---------------------------------------------------------------------------
// First map: artifacts exist, `find` (a separate CLI call) answers.
// ---------------------------------------------------------------------------

#[test]
fn first_map_runs_and_find_answers() {
    let root = init_git_repo_with_source("first-map");
    let env_vars = fresh_env("first-map");
    // No settings.json under this HOME -- the hooks step declines
    // independently of this test's own assertions (failure isolation: a
    // declined hooks step must not block the map step).

    let (out, err, code) = run_init(&root, &[], &env_vars);
    assert_eq!(code, 0, "stdout: {out}\nstderr: {err}");
    assert!(out.contains("map: mapped"), "got: {out}");
    assert!(out.contains("defs"), "got: {out}");
    // The hooks step declining must not have suppressed the map line.
    assert!(out.contains("no settings file"), "got: {out}");

    let manifest_path = root.join(".git/scout/manifest.json");
    assert!(
        manifest_path.is_file(),
        "first map must have written the manifest"
    );
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert!(manifest["entries"]
        .as_object()
        .unwrap()
        .contains_key("src/Widget.cs"));
    assert!(manifest["entries"]
        .as_object()
        .unwrap()
        .contains_key("src/notes.md"));

    let graph_path = root.join(".git/scout/graph/graph.json");
    assert!(
        graph_path.is_file(),
        "graph.json must exist after the first map (a .cs file was present)"
    );

    let (find_out, find_code) = run_find(&root, "Widget", &env_vars);
    assert_eq!(find_code, 0, "{find_out}");
    assert!(
        !find_out.starts_with("no matches"),
        "expected a hit for \"Widget\", got: {find_out}"
    );
}

// ---------------------------------------------------------------------------
// --no-map: skipped, no manifest/graph written by init.
// ---------------------------------------------------------------------------

#[test]
fn no_map_flag_skips_the_first_map() {
    let root = init_git_repo_with_source("no-map-flag");
    let env_vars = fresh_env("no-map-flag");

    let (out, err, code) = run_init(&root, &["--no-map"], &env_vars);
    assert_eq!(code, 0, "stdout: {out}\nstderr: {err}");
    assert!(out.contains("map: skipped (--no-map)"), "got: {out}");

    assert!(
        !root.join(".git/scout/manifest.json").exists(),
        "no manifest should be written when --no-map is passed"
    );
    assert!(
        !root.join(".git/scout/graph").exists(),
        "no graph dir should be created when --no-map is passed"
    );
}

// ---------------------------------------------------------------------------
// Both flags together + the language census line, independent of hooks/map.
// ---------------------------------------------------------------------------

#[test]
fn both_flags_together_skip_hooks_and_map_but_census_still_runs() {
    let root = init_git_repo_with_source("both-flags");
    fs::write(
        root.join("src/component.ts"),
        "export const component = true;\n",
    )
    .unwrap();
    let env_vars = fresh_env("both-flags");
    seed_settings(&env_vars, WELL_FORMED_SETTINGS);
    let path = settings_path(&env_vars);
    let before = fs::read(&path).unwrap();

    let (out, err, code) = run_init(&root, &["--no-hooks", "--no-map"], &env_vars);
    assert_eq!(code, 0, "stdout: {out}\nstderr: {err}");
    assert!(out.contains("hooks: skipped (--no-hooks)"), "got: {out}");
    assert!(out.contains("map: skipped (--no-map)"), "got: {out}");
    assert!(out.contains("languages: 1 .cs (fully supported); 1 .ts (indexed and graphed, narrower edge coverage); 1 .md (present, not indexed)"), "got: {out}");

    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "settings untouched when both flags skip the only step that would touch it"
    );
    assert!(!root.join(".git/scout/manifest.json").exists());
}

// ---------------------------------------------------------------------------
// Failure isolation: a nested-git-repos refusal never reaches the follow-on
// steps at all -- exit code stays the core failure's own nonzero code, output
// stays exactly the core refusal text.
// ---------------------------------------------------------------------------

#[test]
fn core_failure_short_circuits_before_any_follow_on_step_runs() {
    let parent = temp_dir("core-failure");
    for name in ["repo-a", "repo-b"] {
        let r = parent.join(name);
        fs::create_dir_all(&r).unwrap();
        run_git(&r, &["init", "-q"]);
    }
    let env_vars = fresh_env("core-failure");
    seed_settings(&env_vars, WELL_FORMED_SETTINGS);
    let path = settings_path(&env_vars);
    let before = fs::read(&path).unwrap();

    let (out, err, code) = run_init(&parent, &[], &env_vars);
    assert_eq!(code, 2, "stdout: {out}\nstderr: {err}");
    assert!(out.starts_with("refusing to init"), "got: {out}");
    assert!(
        !out.contains("languages:") && !out.contains("hooks:") && !out.contains("map:"),
        "no follow-on step lines on a core failure; got: {out}"
    );

    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "a core failure must never touch settings.json"
    );
    assert!(backup_files(&env_vars).is_empty());
}
