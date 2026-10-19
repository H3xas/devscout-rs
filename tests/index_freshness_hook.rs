//! The read hook's offer under the same rule freshness follows: an offer only
//! when the reading worktree owns the index and the file on disk still hashes
//! to the bytes that index recorded.

#[path = "freshness_fixture/mod.rs"]
mod fixture;

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use fixture::{Fixture, BASELINE};

const EDITED: &str = "namespace App.Widgets\n{\n    public interface IWidget\n    {\n        void Render();\n        void Resize();\n    }\n}\n";

fn full_read_payload(file_path: &Path, body: &str, session: &str) -> String {
    let lines = body.matches('\n').count();
    format!(
        r#"{{"session_id":"{session}","tool_name":"Read","tool_input":{{"file_path":{fp}}},"tool_response":{{"type":"text","file":{{"filePath":{fp},"content":{content},"numLines":{lines},"startLine":1,"totalLines":{lines}}}}}}}"#,
        fp = serde_json::to_string(&file_path.to_string_lossy()).unwrap(),
        content = serde_json::to_string(body).unwrap(),
    )
}

/// Feeds one read payload to `devscout hook read` in `dir` and returns stdout.
fn hook_read(fx: &Fixture, dir: &Path, payload: &str) -> String {
    hook_read_with_path(fx, dir, payload, None)
}

fn hook_read_with_path(
    fx: &Fixture,
    dir: &Path,
    payload: &str,
    path_override: Option<String>,
) -> String {
    let mut cmd = Command::new(fixture::rust_bin());
    cmd.args(["hook", "read"])
        .current_dir(dir)
        .env("HOME", &fx.home)
        .env("SCOUT_REGISTRY", fx.home.join("repos.json"))
        .env("SCOUT_CONTENT_DB", fx.home.join("content.db"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = path_override {
        cmd.env("PATH", path);
    }
    let mut child = cmd.spawn().expect("spawn devscout hook read");
    child
        .stdin
        .as_mut()
        .expect("open stdin")
        .write_all(payload.as_bytes())
        .expect("write stdin payload");
    let out = child.wait_with_output().expect("hook read must run");
    assert_eq!(out.status.code(), Some(0), "hooks fail open: {out:?}");
    String::from_utf8(out.stdout).expect("stdout is utf-8")
}

fn offers(output: &str) -> bool {
    output.contains("this file is indexed")
}

#[test]
fn index_freshness_ac_9_an_offer_comes_from_the_owning_worktree() {
    let (fx, _sha) = Fixture::mapped("ac9-offer", BASELINE);
    let file = fx.root.join("src/IWidget.cs");
    let body = fx.read("src/IWidget.cs");

    let out = hook_read(&fx, &fx.root, &full_read_payload(&file, &body, "s1"));
    assert!(
        offers(&out),
        "a fresh read of an indexed file offers: {out:?}"
    );
}

#[test]
fn index_freshness_ac_9_a_sibling_worktree_gets_no_offer() {
    let (fx, _sha) = Fixture::mapped("ac9-sibling", BASELINE);
    let linked = fx.add_worktree("linked");
    let init = Command::new(fixture::rust_bin())
        .args(["init", "--no-hooks", "--no-map"])
        .current_dir(&linked)
        .env("HOME", &fx.home)
        .env("SCOUT_REGISTRY", fx.home.join("repos.json"))
        .env("SCOUT_CONTENT_DB", fx.home.join("content.db"))
        .output()
        .expect("init in the linked worktree");
    assert!(init.status.success(), "{init:?}");

    let file = linked.join("src/IWidget.cs");
    let body = fs::read_to_string(&file).unwrap();
    let out = hook_read(&fx, &linked, &full_read_payload(&file, &body, "s2"));
    assert!(
        !offers(&out),
        "the index belongs to another worktree, so it certifies nothing here: {out:?}"
    );
}

#[test]
fn index_freshness_ac_9_a_second_edit_of_a_dirty_file_gets_no_offer() {
    let (fx, _sha) = Fixture::mapped("ac9-edit", BASELINE);
    let file = fx.root.join("src/IWidget.cs");
    let body = fx.read("src/IWidget.cs");
    assert!(offers(&hook_read(
        &fx,
        &fx.root,
        &full_read_payload(&file, &body, "s3-1")
    )));

    fx.write("src/IWidget.cs", EDITED);
    let out = hook_read(&fx, &fx.root, &full_read_payload(&file, EDITED, "s3-2"));
    assert!(
        !offers(&out),
        "the file's bytes are no longer the ones the index recorded: {out:?}"
    );
}

#[test]
fn index_freshness_ac_9_a_legacy_record_gets_no_offer() {
    let (fx, sha) = Fixture::mapped("ac9-legacy", BASELINE);
    fx.write_legacy_record(&sha);
    let file = fx.root.join("src/IWidget.cs");
    let body = fx.read("src/IWidget.cs");
    let out = hook_read(&fx, &fx.root, &full_read_payload(&file, &body, "s4"));
    assert!(!offers(&out), "{out:?}");
}

#[test]
fn index_freshness_ac_9_an_in_progress_marker_gets_no_offer() {
    let (fx, _sha) = Fixture::mapped("ac9-marker", BASELINE);
    let owner = fx.owner();
    fx.write_marker(&owner);
    let file = fx.root.join("src/IWidget.cs");
    let body = fx.read("src/IWidget.cs");
    let out = hook_read(&fx, &fx.root, &full_read_payload(&file, &body, "s5"));
    assert!(!offers(&out), "{out:?}");
}

#[test]
fn index_freshness_ac_9_the_head_moved_note_and_the_repeat_read_stub_are_unchanged() {
    let (fx, _sha) = Fixture::mapped("ac9-note", BASELINE);
    let file = fx.root.join("src/IWidget.cs");
    let body = fx.read("src/IWidget.cs");

    let first = hook_read(&fx, &fx.root, &full_read_payload(&file, &body, "s6"));
    assert!(offers(&first), "{first:?}");

    let repeat = hook_read(&fx, &fx.root, &full_read_payload(&file, &body, "s6"));
    assert!(
        repeat.contains("unchanged since first read this session"),
        "a repeat read still stubs: {repeat:?}"
    );

    // A different body, so the repeat-read stub cannot answer first.
    fx.advance_head();
    fx.write("src/IWidget.cs", EDITED);
    let after = hook_read(&fx, &fx.root, &full_read_payload(&file, EDITED, "s6"));
    assert!(
        after.contains("manifest stale — HEAD moved since last map"),
        "the HEAD-moved note is unchanged: {after:?}"
    );
}

#[cfg(unix)]
#[test]
fn index_freshness_ac_9_the_hook_runs_no_git_status_per_read() {
    use std::os::unix::fs::PermissionsExt;

    let (fx, _sha) = Fixture::mapped("ac9-nospawn", BASELINE);
    let real = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("locate git");
    let real = String::from_utf8(real.stdout).unwrap().trim().to_string();

    let shim_dir = fx.base.join("hook-shim");
    fs::create_dir_all(&shim_dir).unwrap();
    let tally = shim_dir.join("status-count");
    let shim = shim_dir.join("git");
    fs::write(
        &shim,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do\n  if [ \"$a\" = \"status\" ]; then\n    echo s >> {tally}\n  fi\ndone\nexec {real} \"$@\"\n",
            tally = tally.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();

    let file = fx.root.join("src/IWidget.cs");
    let body = fx.read("src/IWidget.cs");
    let out = hook_read_with_path(
        &fx,
        &fx.root,
        &full_read_payload(&file, &body, "s7"),
        Some(format!(
            "{}:{}",
            shim_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        )),
    );
    assert!(offers(&out), "{out:?}");
    assert!(
        !tally.exists(),
        "the hook decides from the record and the file's own digest, never from git status"
    );
}
