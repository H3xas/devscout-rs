//! Integration tests for the heuristic TIER on the query surface and `--no-guess`.

// Two guesses, one of each tier, over one mapped fixture repo:
//
// - `WidgetExtensions.Render` is reached through C#'s own extension-method
//   lookup, which can see the `(member, this-type)` pair but not the receiver's
//   real members -- tier `ext`.
// - `Counter.Tally` is reached by the scored tier, which knows only that
//   exactly one type in the graph declares a member of that name -- tier
//   `guess`.
//
// `--no-guess` is the caller saying "facts and language rules only": the
// extension row survives it, the scored one does not.
//
// Every test here goes through the COMPILED BINARY as a subprocess against a
// graph the binary itself mapped, the same discipline cli_bare_member.rs states:
// a tier that never survives a real extraction would otherwise pass vacuously
// against a hand-built fixture graph.
//
// SAFETY: every spawned process gets HOME, SCOUT_REGISTRY and SCOUT_CONTENT_DB
// pointed at a fresh temp dir, so the operator's real settings and registry are
// never read and never written.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-no-guess-{prefix}-{}-{n}",
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

/// `WidgetExtensions` and `Counter` share ONE file so a single `impact` seed
/// reaches both tiers and the two heuristic counts can be compared directly.
///
/// `Widget` is declared but never declares `Render`, so only the extension tier
/// can claim `w.Render()`; `Build()` is declared nowhere, so `x.Tally()` has no
/// receiver type at all and only the scored tier can claim it. The test file
/// makes BOTH calls, which is what proves a row folding one edge of each tier
/// reports the stronger one.
const FILES: &[(&str, &str)] = &[
    (
        "Other/Widget.cs",
        "namespace App.Other { public class Widget { } }\n",
    ),
    (
        "Core/Targets.cs",
        "namespace App.Core\n{\n    public static class WidgetExtensions\n    {\n        public static void Render(this Widget w) { }\n    }\n\n    public class Counter\n    {\n        public void Tally() { }\n    }\n}\n",
    ),
    (
        "Consumers/UsesExtension.cs",
        "\nusing App.Other;\nusing App.Core;\n\nnamespace App.Consumers;\n\npublic class UsesExtension\n{\n  public void Run(Widget w) => w.Render();\n}\n",
    ),
    (
        "Consumers/Guesser.cs",
        "\nnamespace App.Consumers;\n\npublic class Guesser\n{\n  public void Run()\n  {\n    var x = Build();\n    x.Tally();\n  }\n}\n",
    ),
    (
        "Tests/CounterTests.cs",
        "\nusing App.Other;\nusing App.Core;\n\nnamespace App.Tests;\n\npublic class CounterTests\n{\n  [Fact]\n  public void Guesses()\n  {\n    var x = Build();\n    x.Tally();\n  }\n\n  [Fact]\n  public void Extends(Widget w) => w.Render();\n}\n",
    ),
];

struct Fixture {
    root: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn build(prefix: &str) -> Fixture {
        Fixture::build_from(prefix, FILES)
    }

    fn build_from(prefix: &str, files: &[(&str, &str)]) -> Fixture {
        let base = temp_dir(prefix);
        let root = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&home).expect("create home dir");
        for (rel, body) in files {
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

/// `uses-member` edges out of `(from_file, from_line)`, precise (no
/// `heuristic` key) or otherwise, as `(to, member, is precise)`.
fn member_edges_at<'a>(
    graph: &'a serde_json::Value,
    from_file: &str,
    from_line: u64,
) -> Vec<(&'a str, &'a str, bool)> {
    graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["kind"] == "uses-member"
                && e["from_file"] == from_file
                && e["from_line"].as_u64() == Some(from_line)
        })
        .map(|e| {
            (
                e["to"].as_str().unwrap(),
                e["member"].as_str().unwrap_or(""),
                e["heuristic"].is_null(),
            )
        })
        .collect()
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

#[test]
fn refs_names_each_tier_with_its_own_word_and_marks_it_with_its_own_compact_character() {
    let fx = Fixture::build("words");

    let ext = stdout_of(&fx.run(&["refs", "WidgetExtensions"]));
    assert!(
        ext.contains(
            "    Consumers/UsesExtension.cs:9  uses-member (extension)  public void Run(Widget w) => w.Render();"
        ),
        "{ext}"
    );
    let guess = stdout_of(&fx.run(&["refs", "Counter"]));
    assert!(
        guess.contains("    Consumers/Guesser.cs:9  uses-member (guess)  x.Tally();"),
        "{guess}"
    );
    // The umbrella word is now the HEADER's alone: no row falls back to it once
    // every heuristic edge in the graph carries a tier.
    assert!(!ext.contains("(heuristic)"), "{ext}");
    assert!(!guess.contains("(heuristic)"), "{guess}");

    let ext_compact = stdout_of(&fx.run(&["refs", "WidgetExtensions", "--compact"]));
    assert!(
        ext_compact.contains(
            "in:uses-member (2):\n  Consumers/UsesExtension.cs:9x\n  Tests/CounterTests.cs:17x"
        ),
        "{ext_compact}"
    );
    let guess_compact = stdout_of(&fx.run(&["refs", "Counter", "--compact"]));
    assert!(
        guess_compact.contains(
            "in:uses-member (2):\n  Consumers/Guesser.cs:9h\n  Tests/CounterTests.cs:13h"
        ),
        "{guess_compact}"
    );
}

#[test]
fn refs_json_carries_the_tier_next_to_the_flag_it_refines() {
    let fx = Fixture::build("json");

    let ext = stdout_of(&fx.run(&["refs", "WidgetExtensions", "--json"]));
    assert!(
        ext.contains(
            r#"{"file":"Consumers/UsesExtension.cs","line":9,"heuristic":true,"tier":"ext","source":"public void Run(Widget w) => w.Render();","why":"uses-member-ext"}"#
        ),
        "{ext}"
    );
    let guess = stdout_of(&fx.run(&["refs", "Counter", "--json"]));
    assert!(
        guess.contains(
            r#"{"file":"Consumers/Guesser.cs","line":9,"heuristic":true,"tier":"guess","source":"x.Tally();","why":"uses-member-guess"}"#
        ),
        "{guess}"
    );
}

#[test]
fn read_and_tests_carry_the_tier_in_compact_and_json_like_refs() {
    let fx = Fixture::build("read-tests-tier");

    // `read` shares the refs table renderers, so every format marks the tier
    // the same way refs does.
    let ext_compact = stdout_of(&fx.run(&["read", "WidgetExtensions", "--compact"]));
    assert!(
        ext_compact.contains("Consumers/UsesExtension.cs:9x"),
        "{ext_compact}"
    );
    let guess_compact = stdout_of(&fx.run(&["read", "Counter", "--compact"]));
    assert!(
        guess_compact.contains("Consumers/Guesser.cs:9h"),
        "{guess_compact}"
    );

    let ext_json = stdout_of(&fx.run(&["read", "WidgetExtensions", "--json"]));
    assert!(
        ext_json.contains(
            r#"{"file":"Consumers/UsesExtension.cs","line":9,"heuristic":true,"tier":"ext","source":"public void Run(Widget w) => w.Render();","why":"uses-member-ext"}"#
        ),
        "{ext_json}"
    );
    let guess_json = stdout_of(&fx.run(&["read", "Counter", "--json"]));
    assert!(
        guess_json.contains(
            r#"{"file":"Consumers/Guesser.cs","line":9,"heuristic":true,"tier":"guess","source":"x.Tally();","why":"uses-member-guess"}"#
        ),
        "{guess_json}"
    );

    // `tests` rows carry the same pair in the same order.
    let tests_ext = stdout_of(&fx.run(&["tests", "WidgetExtensions", "--json"]));
    assert!(
        tests_ext.contains(r#""heuristic":true,"tier":"ext""#),
        "{tests_ext}"
    );
    assert!(!tests_ext.contains(r#""tier":"guess""#), "{tests_ext}");
    let tests_guess = stdout_of(&fx.run(&["tests", "Counter", "--json"]));
    assert!(
        tests_guess.contains(r#""heuristic":true,"tier":"guess""#),
        "{tests_guess}"
    );
    assert!(!tests_guess.contains(r#""tier":"ext""#), "{tests_guess}");
}

#[test]
fn no_guess_keeps_the_extension_row_and_drops_the_scored_one_in_refs_and_read() {
    let fx = Fixture::build("refs-read");

    for verb in ["refs", "read"] {
        let ext = stdout_of(&fx.run(&[verb, "WidgetExtensions", "--no-guess"]));
        assert!(
            ext.contains("  uses-member (2):"),
            "the extension tier is a language rule, not a name guess: {verb}\n{ext}"
        );
        assert!(
            ext.contains("Consumers/UsesExtension.cs:9  uses-member (extension)"),
            "{verb}\n{ext}"
        );

        let guess = stdout_of(&fx.run(&[verb, "Counter", "--no-guess"]));
        assert!(
            guess.contains("  uses-member (0):"),
            "every scored row is gone, and the table says zero rather than vanishing: {verb}\n{guess}"
        );
        assert!(!guess.contains("Guesser.cs"), "{verb}\n{guess}");

        // The symbol itself still resolves -- `--no-guess` narrows the ANSWER,
        // it does not turn a known type into a zero hit.
        let out = fx.run(&[verb, "Counter", "--no-guess"]);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
    }
}

#[test]
fn no_guess_leaves_a_test_file_reached_by_an_extension_call_and_drops_one_reached_by_a_guess() {
    let fx = Fixture::build("tests");

    let ext = stdout_of(&fx.run(&["tests", "WidgetExtensions", "--no-guess"]));
    assert!(ext.contains("Tests/CounterTests.cs (extension)"), "{ext}");
    assert_eq!(
        stdout_of(&fx.run(&["tests", "WidgetExtensions", "--no-guess", "--compact"])),
        "tests App.Core.WidgetExtensions files=0 refs=0 heuristic=1\nTests/CounterTests.cs 17x\n"
    );

    let guess = stdout_of(&fx.run(&["tests", "Counter"]));
    assert!(guess.contains("Tests/CounterTests.cs (guess)"), "{guess}");
    assert_eq!(
        stdout_of(&fx.run(&["tests", "Counter", "--no-guess"])),
        "tests for App.Core.Counter\nno test references found\n",
        "nothing but a scored guess reached this symbol from a test file"
    );
}

#[test]
fn impact_no_guess_counts_only_the_files_an_extension_call_reached() {
    let fx = Fixture::build("impact");

    let full = stdout_of(&fx.run(&["impact", "Core/Targets.cs"]));
    assert!(
        full.contains("affected files: 0 (+3 heuristic)  shown: 3  dropped: 0"),
        "{full}"
    );
    assert!(
        full.contains("Consumers/Guesser.cs  1  1  Counter (guess)"),
        "{full}"
    );
    assert!(
        full.contains("Consumers/UsesExtension.cs  1  1  WidgetExtensions (extension)"),
        "{full}"
    );
    // One extension edge and one scored edge reach this file; the stronger tier
    // names the row.
    assert!(
        full.contains("Tests/CounterTests.cs  1  2  WidgetExtensions, Counter (extension)"),
        "{full}"
    );

    let narrowed = stdout_of(&fx.run(&["impact", "Core/Targets.cs", "--no-guess"]));
    assert!(
        narrowed.contains("affected files: 0 (+2 heuristic)  shown: 2  dropped: 0"),
        "{narrowed}"
    );
    assert!(!narrowed.contains("Guesser.cs"), "{narrowed}");

    let json = stdout_of(&fx.run(&["impact", "Core/Targets.cs", "--no-guess", "--json"]));
    assert!(
        json.contains(r#""heuristicAffected":2"#),
        "only the ext-reached files are counted: {json}"
    );
    assert!(
        json.contains(
            r#""score":0,"heuristicCount":1,"heuristic":true,"tier":"ext","fromLines":{"heuristic":9}"#
        ),
        "{json}"
    );
    assert!(!json.contains(r#""tier":"guess""#), "{json}");

    let compact = stdout_of(&fx.run(&["impact", "Core/Targets.cs", "--compact"]));
    assert!(
        compact.contains("  Consumers/Guesser.cs via=1h"),
        "{compact}"
    );
    assert!(
        compact.contains("  Consumers/UsesExtension.cs via=1x"),
        "{compact}"
    );
}

/// `Binder`/`BinderConsumer` are the external-base shape: a property typed
/// `IBinder`, whose own base is external, shadowed by an unrelated
/// same-named in-tree class in the same namespace. `Journal`/
/// `JournalConsumer` are the cross-project shape: a property typed
/// `IJournal` (which declares `Record` itself), shadowed by an unrelated
/// same-named in-tree class reached only through a `using`.
/// Both shadowing types are found by the SAME bare identifier as the
/// property/field NAME, never the property's own DECLARED type -- the
/// "Color color" collision the arm must no longer resolve through.
const SHADOW_FILES: &[(&str, &str)] = &[
    (
        "Contracts/Contracts.cs",
        "namespace App.Ext.Contracts\n{\n    public interface IBinder : IDisposable\n    {\n    }\n\n    public interface IJournal\n    {\n        void Record(string entry);\n    }\n}\n",
    ),
    (
        "Downstream/Journal.cs",
        "using App.Ext.Contracts;\n\nnamespace App.Downstream\n{\n    public class Journal : IJournal\n    {\n        public void Record(string entry) { }\n    }\n}\n",
    ),
    (
        "Domain/Domain.cs",
        "using App.Downstream;\nusing App.Ext.Contracts;\n\nnamespace App.Domain\n{\n    public class Binder\n    {\n        public void Dispose() { }\n    }\n\n    public class BinderConsumer\n    {\n        public IBinder Binder { get; }\n\n        public void Close() => Binder?.Dispose();\n    }\n\n    public class JournalConsumer\n    {\n        public IJournal Journal { get; }\n\n        public JournalConsumer(IJournal journal)\n        {\n            Journal = journal;\n        }\n\n        public void Write() => Journal.Record(\"x\");\n    }\n}\n",
    ),
];

#[test]
fn a_property_shadowing_a_same_named_type_binds_the_propertys_declared_type_not_the_type() {
    let fx = Fixture::build_from("shadow", SHADOW_FILES);
    let graph = fx.graph();

    // `Binder?.Dispose()` (line 15): the property's declared type `IBinder`
    // does not itself declare `Dispose` -- it only inherits it from the
    // external `IDisposable` -- so the call must stay fully external, never
    // falling back to the same-named in-tree `Binder` class the bare
    // property name happens to also name.
    assert!(
        member_edges_at(&graph, "Domain/Domain.cs", 15).is_empty(),
        "a property whose own interface fails to declare the member must not fall back to a same-named type: {graph:#}"
    );

    // `Journal.Record("x")` (line 27): the property's declared type
    // `IJournal` declares `Record` directly, and must bind THAT, never the
    // same-named downstream `Journal` class the bare property name also
    // reaches through a `using`.
    let journal_edges = member_edges_at(&graph, "Domain/Domain.cs", 27);
    assert_eq!(
        journal_edges,
        vec![("App.Ext.Contracts.IJournal", "Record", true)],
        "a property shadowed by a same-named type must bind the property's OWN declared type: {graph:#}"
    );
}
