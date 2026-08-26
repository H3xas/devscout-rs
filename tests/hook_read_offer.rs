// The read hook's first-read offer: a fresh, whole-file read of a mapped file
// in a fresh index earns exactly one `additionalContext` line naming
// `devscout read <symbol>`; every other path keeps today's behaviour --
// repeat reads stub as before, a stale index notes once then goes silent and
// never offers, an unmapped file stays empty.
//
// Every test here goes through the COMPILED BINARY as a subprocess feeding
// stdin -- the hook contract is byte-shaped stdout for a byte-shaped stdin
// payload, which only exists across the process boundary. Same rule as
// tests/freshness.rs, whose fixture helpers below are duplicated deliberately
// (each tests/*.rs file compiles as an independent binary).
//
// SAFETY: every spawned process gets HOME, SCOUT_REGISTRY and SCOUT_CONTENT_DB
// pointed at a fresh temp dir, so the operator's real ~/.claude/settings.json,
// repos.json and content store are never read and never written.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-hook-offer-{prefix}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn rust_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_devscout"))
}

fn git(dir: &Path, args: &[&str]) -> String {
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

// Line-numbered shapes:
//
// src/IThing.cs            src/Paired.cs
// 1 namespace Shop         1 namespace App
// 2 {                      2 {
// 3     public interface   3     public class First     <- span 3-5
// 4     {                  4     {
// 5         int Id ...     5     }
// 6     }                  6 (blank)
// 7 }                      7     public class Second    <- span 7-9
//                          8     {
//                          9     }
//                          10 }
//
// `notes.txt` is unmapped on purpose: `.txt` is not a source extension, so no
// def can ever name this file and the offer must stay silent.
const ITHING: &str =
    "namespace Shop\n{\n    public interface IThing\n    {\n        int Id { get; }\n    }\n}\n";
const PAIRED: &str = "namespace App\n{\n    public class First\n    {\n    }\n\n    public class Second\n    {\n    }\n}\n";

const FILES: &[(&str, &str)] = &[
    ("src/IThing.cs", ITHING),
    ("src/Paired.cs", PAIRED),
    ("src/notes.txt", "scratch\n"),
];

struct Fixture {
    root: PathBuf,
    home: PathBuf,
}

impl Fixture {
    // Files are committed here (`add` + `write-tree` + `commit-tree`, never
    // `git commit`) so `advance_head` can move HEAD over the SAME tree below
    // without dragging working-tree noise into the staleness assertions.
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
        git(&root, &["init", "-q", "."]);
        git(&root, &["add", "-A"]);
        let tree = git(&root, &["write-tree"]);
        let sha = git(&root, &["commit-tree", &tree, "-m", "init"]);
        git(&root, &["update-ref", "refs/heads/master", &sha]);
        git(&root, &["symbolic-ref", "HEAD", "refs/heads/master"]);

        let fixture = Fixture { root, home };
        let init = fixture.run_raw(&["init", "--no-hooks", "--no-map"]);
        assert!(init.status.success(), "init failed: {init:?}");
        let map = fixture.run_raw(&["map", "."]);
        assert!(map.status.success(), "map failed: {map:?}");
        fixture
    }

    fn run_raw(&self, args: &[&str]) -> Output {
        Command::new(rust_bin())
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .stdin(Stdio::null())
            .output()
            .expect("devscout must run")
    }

    fn advance_head(&self, parent_sha: &str) -> String {
        let tree = git(
            &self.root,
            &["rev-parse", &format!("{parent_sha}^{{tree}}")],
        );
        let sha = git(
            &self.root,
            &["commit-tree", &tree, "-p", parent_sha, "-m", "advance"],
        );
        git(&self.root, &["update-ref", "refs/heads/master", &sha]);
        sha
    }

    fn head_sha(&self) -> String {
        git(&self.root, &["rev-parse", "HEAD"])
    }

    // Feeds one PostToolUse payload to `devscout hook read` on stdin.
    fn hook_read(&self, payload: &str) -> String {
        let mut child = Command::new(rust_bin())
            .args(["hook", "read"])
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn devscout hook read");
        child
            .stdin
            .as_mut()
            .expect("open stdin")
            .write_all(payload.as_bytes())
            .expect("write stdin payload");
        let out = child.wait_with_output().expect("hook read must run");
        assert_eq!(
            out.status.code(),
            Some(0),
            "hooks fail open, exit 0: {out:?}"
        );
        String::from_utf8(out.stdout).expect("stdout is utf-8")
    }
}

// A harness-shaped Read payload whose response reports the WHOLE file.
fn full_read_payload(file_path: &Path, body: &str, session: &str) -> String {
    let lines = body.matches('\n').count();
    format!(
        r#"{{"session_id":"{session}","tool_name":"Read","tool_input":{{"file_path":{fp}}},"tool_response":{{"type":"text","file":{{"filePath":{fp},"content":{content},"numLines":{lines},"startLine":1,"totalLines":{lines}}}}}}}"#,
        fp = serde_json::to_string(&file_path.to_string_lossy()).unwrap(),
        content = serde_json::to_string(body).unwrap(),
    )
}

fn ranged_read_payload(
    file_path: &Path,
    body: &str,
    session: &str,
    start: usize,
    count: usize,
) -> String {
    let total = body.matches('\n').count();
    let content = body
        .lines()
        .skip(start - 1)
        .take(count)
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"{{"session_id":"{session}","tool_name":"Read","tool_input":{{"file_path":{fp},"offset":{start},"limit":{count}}},"tool_response":{{"type":"text","file":{{"filePath":{fp},"content":{content},"numLines":{count},"startLine":{start},"totalLines":{total}}}}}}}"#,
        fp = serde_json::to_string(&file_path.to_string_lossy()).unwrap(),
        content = serde_json::to_string(&content).unwrap(),
    )
}

// The legacy text-only response shape: no line bounds at all, which the offer
// treats as a whole-file read.
fn legacy_read_payload(file_path: &Path, body: &str, session: &str) -> String {
    format!(
        r#"{{"session_id":"{session}","tool_name":"Read","tool_input":{{"file_path":{fp}}},"tool_response":{{"text":{content}}}}}"#,
        fp = serde_json::to_string(&file_path.to_string_lossy()).unwrap(),
        content = serde_json::to_string(body).unwrap(),
    )
}

fn offer_envelope(subject: &str) -> String {
    format!(
        r#"{{"hookSpecificOutput":{{"hookEventName":"PostToolUse","additionalContext":"[devscout: this file is indexed — 'devscout read {subject}' shows its declaration span and inbound references.]"}}}}"#
    )
}

#[test]
fn first_full_read_of_a_mapped_file_names_the_symbol_and_nothing_else() {
    let fx = Fixture::build("offer");
    let out = fx.hook_read(&full_read_payload(
        &fx.root.join("src").join("IThing.cs"),
        ITHING,
        "s1",
    ));
    assert_eq!(
        out,
        offer_envelope("IThing"),
        "the offer rides alone in additionalContext"
    );
}

#[test]
fn first_ranged_read_offers_the_symbol_nearest_the_requested_range() {
    let fx = Fixture::build("ranged-offer");
    let out = fx.hook_read(&ranged_read_payload(
        &fx.root.join("src").join("Paired.cs"),
        PAIRED,
        "s1",
        7,
        3,
    ));
    assert_eq!(out, offer_envelope("Second"));
}

#[test]
fn a_repeat_identical_read_gets_the_stub_instead_of_another_offer() {
    let fx = Fixture::build("repeat");
    let payload = full_read_payload(&fx.root.join("src").join("IThing.cs"), ITHING, "s1");
    assert_eq!(fx.hook_read(&payload), offer_envelope("IThing"));

    let second = fx.hook_read(&payload);
    assert!(
        second.contains("\"updatedToolOutput\""),
        "the repeat dedupes into a stub: {second:?}"
    );
    assert!(
        second.contains("unchanged since first read this session"),
        "{second:?}"
    );
    assert!(
        !second.contains("devscout read"),
        "a stubbed repeat never re-offers: {second:?}"
    );
}

#[test]
fn a_stale_index_notes_once_then_goes_silent_and_never_offers() {
    let fx = Fixture::build("stale");
    fx.advance_head(&fx.head_sha());

    // First stale read of this content: the one-time note.
    let path = fx.root.join("src").join("IThing.cs");
    let first = fx.hook_read(&full_read_payload(&path, ITHING, "sA"));
    assert_eq!(
        first,
        r#"{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"[devscout: manifest stale — HEAD moved since last map. Run 'devscout map --refresh'.]"}}"#,
        "the staleness note replaces any offer"
    );

    // Second stale read, fresh session (so it reaches the gate at all rather
    // than stubbing): the flag is up, output is empty -- never an offer.
    let second = fx.hook_read(&full_read_payload(&path, ITHING, "sB"));
    assert_eq!(second, "", "once flagged stale, silence: {second:?}");
}

#[test]
fn an_unmapped_files_first_read_stays_empty() {
    let fx = Fixture::build("unmapped");
    let out = fx.hook_read(&full_read_payload(
        &fx.root.join("src").join("notes.txt"),
        "scratch\n",
        "s1",
    ));
    assert_eq!(out, "", "nothing mapped in this file, nothing to offer");
}

#[test]
fn whole_file_reads_offer_the_first_mapped_symbol_in_both_response_shapes() {
    let fx = Fixture::build("paired");

    // Modern file shape, bounds present: the whole-file range overlaps both
    // spans; the tie-break picks the earlier start line.
    let modern = fx.hook_read(&full_read_payload(
        &fx.root.join("src").join("Paired.cs"),
        PAIRED,
        "s1",
    ));
    assert_eq!(modern, offer_envelope("First"));

    // Legacy text-only shape, no bounds at all: documented fallback is the
    // file's FIRST mapped symbol, which is the same answer here.
    let legacy = fx.hook_read(&legacy_read_payload(
        &fx.root.join("src").join("Paired.cs"),
        PAIRED,
        "s2",
    ));
    assert_eq!(legacy, offer_envelope("First"));
}
