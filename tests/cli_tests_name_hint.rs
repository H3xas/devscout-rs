//! Integration tests for the `tests` file-name hint.
//!
//! A symbol nothing tests answers zero rows. When a test file in the repo is
//! NAMED after that symbol, the answer says so and names the production
//! symbols that file references instead -- the observation a caller needs to
//! stop reading an empty answer as "this symbol is tested somewhere else".
//!
//! The hint is a file-name observation, so these tests also pin what it must
//! NOT do: a symbol with a real reference gets no hint at all, and the hint
//! changes no row, no count, no outcome and no exit code. The comparison
//! fixture for that last rule differs from the hinted one in the test file's
//! NAME alone.
//!
//! Every test goes through the COMPILED BINARY as a subprocess against a graph
//! the binary itself mapped: a name rule that never survives a real extraction
//! would otherwise pass vacuously against a hand-built fixture graph.
//!
//! SAFETY: every spawned process gets HOME, SCOUT_REGISTRY and SCOUT_CONTENT_DB
//! pointed at a fresh temp dir, so the operator's real settings and registry are
//! never read and never written.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-tests-hint-{prefix}-{}-{n}",
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

// `Foo` is the untested symbol, `Bar` is what the same-named test file really
// references, and the test file's own name is the misleading fact: it matches
// `Foo` while its code never mentions `Foo` at all.
const FOOTESTS: &str = "\nnamespace App.Tests;\n\npublic class FooTests\n{\n  [Fact]\n  public void Builds()\n  {\n    var b = new Bar();\n  }\n}\n";

fn files_with(test_file: &str) -> Vec<(String, String)> {
    vec![
        (
            "Src/Foo.cs".to_string(),
            "namespace App.Src { public class Foo { } }\n".to_string(),
        ),
        (
            "Src/Bar.cs".to_string(),
            "namespace App.Src { public class Bar { } }\n".to_string(),
        ),
        (test_file.to_string(), FOOTESTS.to_string()),
    ]
}

struct Fixture {
    root: PathBuf,
    home: PathBuf,
}

impl Fixture {
    /// A repo whose test file is named after `Foo`.
    fn named_after_the_symbol() -> Fixture {
        Fixture::build("hint", "Tests/FooTests.cs")
    }

    /// The same repo with the test file renamed so no name matches `Foo`.
    fn twin_with_no_matching_name() -> Fixture {
        Fixture::build("twin", "Tests/BarBuilderTests.cs")
    }

    fn build(prefix: &str, test_file: &str) -> Fixture {
        let base = temp_dir(prefix);
        let root = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&home).expect("create home dir");
        for (rel, body) in files_with(test_file) {
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

    fn graph(&self) -> serde_json::Value {
        let text = fs::read_to_string(self.root.join(".git/scout/graph/graph.json"))
            .expect("graph.json must exist after map");
        serde_json::from_str(&text).unwrap()
    }
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("stderr is utf-8")
}

fn stdout_json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout_of(out)).expect("the answer must be json")
}

// The kinds of `from_file -> to` edges the graph records without a heuristic
// marker. A `new Bar()` the extractor resolved by name alone would land in the
// heuristic adjacency instead, and the hint lists precise edges only, so the
// fixture has to prove its reference is one of these before any assertion
// about the hint's contents means anything.
fn precise_kinds(graph: &serde_json::Value, from_file: &str, to: &str) -> Vec<String> {
    graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["from_file"] == from_file && e["to"] == to && e["heuristic"].is_null())
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect()
}

fn def_id_named(graph: &serde_json::Value, name: &str) -> String {
    graph["defs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == name)
        .unwrap_or_else(|| panic!("no def named {name} in the fixture graph"))["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn assert_hint_text_says_what_it_is(text: &str) {
    assert!(
        text.contains("hint (file name only, not a test reference):"),
        "the heading states the block is a name, not a reference: {text}"
    );
}

#[test]
fn tests_name_hint_ac_1_no_reference_answer_names_a_same_named_test_file() {
    let fx = Fixture::named_after_the_symbol();
    let bar = def_id_named(&fx.graph(), "Bar");
    let kinds = precise_kinds(&fx.graph(), "Tests/FooTests.cs", &bar);
    assert!(
        !kinds.is_empty(),
        "the fixture must reference Bar over a precise edge, not a guess: {kinds:?}"
    );

    let out = fx.run(&["tests", "Foo"]);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(0), "{out:?} {}", stderr_of(&out));
    assert!(text.contains("0 files"), "{text}");
    assert!(text.contains("no test references found"), "{text}");
    assert_hint_text_says_what_it_is(&text);
    assert!(
        text.contains(&format!("  Tests/FooTests.cs  references {bar}")),
        "{text}"
    );

    let json = stdout_json(&fx.run(&["tests", "Foo", "--json"]));
    assert_eq!(json["outcome"], "hit", "{json}");
    assert_eq!(json["nameHint"][0]["file"], "Tests/FooTests.cs", "{json}");
    assert_eq!(json["nameHint"][0]["references"][0], bar.as_str(), "{json}");
    assert_eq!(json["nameHint"][0]["referenceTotal"], 1, "{json}");
}

#[test]
fn tests_name_hint_ac_2_a_real_reference_answer_carries_no_hint() {
    let fx = Fixture::named_after_the_symbol();

    let out = fx.run(&["tests", "Bar"]);
    let text = stdout_of(&out);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(text.contains("Tests/FooTests.cs"), "{text}");
    assert!(!text.contains("hint ("), "{text}");

    let compact = stdout_of(&fx.run(&["tests", "Bar", "--compact"]));
    assert!(compact.contains("Tests/FooTests.cs"), "{compact}");
    assert!(!compact.contains("hint "), "{compact}");

    let json = stdout_json(&fx.run(&["tests", "Bar", "--json"]));
    assert_eq!(json["outcome"], "hit", "{json}");
    assert!(
        json.get("nameHint").is_none(),
        "a row is an answer, so no name observation joins it: {json}"
    );
}

#[test]
fn tests_name_hint_ac_3_the_hint_changes_nothing_but_its_own_block() {
    let hinted = Fixture::named_after_the_symbol();
    let twin = Fixture::twin_with_no_matching_name();

    let bar = def_id_named(&hinted.graph(), "Bar");
    let heading = "hint (file name only, not a test reference):\n";
    let row = format!("  Tests/FooTests.cs  references {bar}\n");
    let hinted_text = stdout_of(&hinted.run(&["tests", "Foo"]));
    let twin_text = stdout_of(&twin.run(&["tests", "Foo"]));
    assert!(hinted_text.contains(heading), "{hinted_text}");
    assert!(
        hinted_text.replace(heading, "").replace(&row, "") == twin_text,
        "the heading and the hint rows are the whole difference\n{hinted_text}\n{twin_text}"
    );

    let hinted_text_out = hinted.run(&["tests", "Foo"]);
    let twin_text_out = twin.run(&["tests", "Foo"]);
    assert_eq!(hinted_text_out.status.code(), twin_text_out.status.code());

    let hinted_out = hinted.run(&["tests", "Foo", "--json"]);
    let twin_out = twin.run(&["tests", "Foo", "--json"]);
    assert_eq!(hinted_out.status.code(), twin_out.status.code());

    let hinted_json = stdout_of(&hinted_out);
    let twin_json = stdout_of(&twin_out);
    let member = format!(
        r#""nameHint":[{{"file":"Tests/FooTests.cs","references":["{bar}"],"referenceTotal":1}}],"#
    );
    assert!(hinted_json.contains(&member), "{hinted_json}");
    assert_eq!(
        hinted_json.replace(&member, ""),
        twin_json,
        "the json answers must be equal once the hint member is removed"
    );

    let hinted_value = stdout_json(&hinted_out);
    let twin_value = stdout_json(&twin_out);
    for key in [
        "rows",
        "testFileCount",
        "refCount",
        "heuristicFileCount",
        "heuristicRefCount",
        "outcome",
        "next",
        "schema_version",
    ] {
        assert_eq!(hinted_value[key], twin_value[key], "{key}");
    }
}

#[test]
fn tests_name_hint_lists_every_matching_file_in_path_order() {
    let fx = Fixture::named_after_the_symbol();
    let second = fx.root.join("Tests/Deep/FooTests.cs");
    fs::create_dir_all(second.parent().unwrap()).unwrap();
    fs::write(&second, FOOTESTS).unwrap();
    assert!(fx.run(&["map", "."]).status.success());

    let json = stdout_json(&fx.run(&["tests", "Foo", "--json"]));
    let files: Vec<&str> = json["nameHint"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["file"].as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["Tests/Deep/FooTests.cs", "Tests/FooTests.cs"]);
}
