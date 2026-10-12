//! Integration tests for the two things every query answer now states: how many
//! rows it counted against how many it printed, and which verb to run next.
//!
//! Every test goes through the COMPILED BINARY as a subprocess, because the
//! claim under test is about the bytes a caller reads -- the first line's count
//! and stdout's last line -- and an in-process call on `cmd_*` could not tell
//! the two apart.
//!
//! SAFETY: every spawned process gets `HOME`, `SCOUT_REGISTRY` and
//! `SCOUT_CONTENT_DB` pointed at a fresh temp dir, so the operator's real
//! `~/.claude/settings.json` and `repos.json` are never read and never written.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

// Sources a test adds on top of the committed fixture, so the quoting rule
// reaches a repository-relative path that carries a space without committing
// one.
const SPACED_SOURCES: &[(&str, &str)] = &[
    (
        "Spaced Dir/ILedger.cs",
        "namespace Toolworks.Ledgers;\n\npublic interface ILedger\n{\n    void Post(string entry);\n}\n",
    ),
    (
        "Spaced Dir/LedgerReader.cs",
        "namespace Toolworks.Ledgers;\n\npublic class LedgerReader\n{\n    public void Read(ILedger ledger) => ledger.Post(\"entry\");\n}\n",
    ),
];

// A resource key matching the query. It is a declaration row of the block but
// names nothing `read` opens, so it never carries the trailer itself.
const RESOURCE_KEY_SOURCE: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<root>\n  <data name=\"Panel\" xml:space=\"preserve\">\n    <value>Panel</value>\n  </data>\n</root>\n";

// The class declaring the same name, two lines in so its row is recognisable.
const PANEL_CLASS_SOURCE: &str =
    "namespace Demo;\n\npublic class Panel\n{\n    public void Show() { }\n}\n";

// A name whose matching rows are not all declarations: a markup class and a
// resource key that both match the query, printed ahead of the C# class that
// declares the same name. `read` opens only the class.
const MIXED_POOL_SOURCES: &[(&str, &str)] = &[
    ("src/AStrings.resw", RESOURCE_KEY_SOURCE),
    (
        "src/AView.xaml",
        "<UserControl x:Class=\"Demo.Panel\">\n</UserControl>\n",
    ),
    ("src/ZPanel.cs", PANEL_CLASS_SOURCE),
];

// A `(&str, &str)` source list as the owned pairs a generated list needs.
fn owned(sources: &[(&str, &str)]) -> Vec<(String, String)> {
    sources
        .iter()
        .map(|(path, body)| ((*path).to_string(), (*body).to_string()))
        .collect()
}

// One resource key per row ahead of the class, so the class declaration sits
// at row `resource_keys + 1` of the declaration block.
fn capped_pool_sources(resource_keys: usize) -> Vec<(String, String)> {
    let mut sources: Vec<(String, String)> = (0..resource_keys)
        .map(|i| (format!("src/A{i:02}.resw"), RESOURCE_KEY_SOURCE.to_string()))
        .collect();
    sources.push(("src/ZPanel.cs".to_string(), PANEL_CLASS_SOURCE.to_string()));
    sources
}

struct Fixture {
    root: PathBuf,
    home: PathBuf,
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create fixture dir");
    for entry in fs::read_dir(from).expect("read fixture dir") {
        let entry = entry.expect("fixture entry");
        let destination = to.join(entry.file_name());
        if entry.file_type().expect("fixture entry type").is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).expect("copy fixture file");
        }
    }
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
    for args in [
        vec!["update-ref", "refs/heads/master", &sha],
        vec!["symbolic-ref", "HEAD", "refs/heads/master"],
    ] {
        let status = Command::new("git")
            .args(&args)
            .current_dir(dir)
            .status()
            .expect("git binary must be on PATH");
        assert!(status.success(), "git {args:?} failed");
    }
}

impl Fixture {
    fn build(label: &str, extra: Vec<(String, String)>) -> Fixture {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let base = env::temp_dir().join(format!(
            "scout-next-hints-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&home).expect("create home dir");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-next-hints");
        copy_tree(&fixture, &root);
        for (rel, body) in &extra {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).expect("create extra dir");
            fs::write(path, body).expect("write extra source");
        }
        let status = Command::new("git")
            .args(["init", "-q", "."])
            .current_dir(&root)
            .status()
            .expect("git binary must be on PATH");
        assert!(status.success(), "git init failed");
        bootstrap_initial_commit(&root);

        let fx = Fixture { root, home };
        let init = fx.run(&["init", "--no-hooks"]);
        assert!(init.status.success(), "init failed: {init:?}");
        fx
    }

    fn plain(label: &str) -> Fixture {
        Fixture::build(label, Vec::new())
    }

    fn with_spaced_paths(label: &str) -> Fixture {
        Fixture::build(label, owned(SPACED_SOURCES))
    }

    fn with_mixed_pool(label: &str) -> Fixture {
        Fixture::build(label, owned(MIXED_POOL_SOURCES))
    }

    fn with_capped_pool(label: &str, resource_keys: usize) -> Fixture {
        Fixture::build(label, capped_pool_sources(resource_keys))
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .output()
            .expect("devscout must run")
    }

    fn stdout(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(out.status.success(), "{args:?} failed: {out:?}");
        stdout_of(&out)
    }

    /// The same, for an answer that leaves on its own non-zero code -- a
    /// zero-hit or a refused seed still prints a complete answer.
    fn answer(&self, args: &[&str]) -> String {
        stdout_of(&self.run(args))
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        let mut with_json = args.to_vec();
        with_json.push("--json");
        serde_json::from_str(&self.stdout(&with_json)).expect("a --json answer")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(base) = self.root.parent() {
            let _ = fs::remove_dir_all(base);
        }
    }
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

fn first_line(text: &str) -> &str {
    text.lines()
        .next()
        .expect("an answer prints at least one line")
}

fn last_line(text: &str) -> &str {
    text.lines()
        .last()
        .expect("an answer prints at least one line")
}

fn suggestion_argv(value: &serde_json::Value) -> Vec<Vec<String>> {
    value["next"]
        .as_array()
        .map(|argvs| {
            argvs
                .iter()
                .map(|argv| {
                    argv.as_array()
                        .expect("an argv array")
                        .iter()
                        .map(|arg| arg.as_str().expect("an argv string").to_string())
                        .collect()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The `next:` trailer split back into argvs: the line is written for a POSIX
/// shell, so undoing that quoting is the only way to check it says what it
/// appears to say.
fn suggestions_from_text(text: &str) -> Vec<Vec<String>> {
    let Some(line) = text.lines().last().filter(|l| l.starts_with("next: ")) else {
        return Vec::new();
    };
    line["next: ".len()..]
        .split(" | ")
        .map(|command| {
            let mut words = split_posix(command);
            assert_eq!(
                words.first().map(String::as_str),
                Some("devscout"),
                "every suggested command names the binary: {command}"
            );
            words.remove(0);
            words
        })
        .collect()
}

fn split_posix(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if !quoted => {
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                    started = true;
                }
            }
            ' ' | '\t' if !quoted => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            _ => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(current);
    }
    words
}

/// Every suggestion has to run. Exit 0 is the ordinary hit; exit 3 is the
/// answer's own "resolved, and empty", which a suggestion naming a seed nothing
/// references legitimately reaches. Anything else -- an ambiguous seed, a usage
/// error -- is a suggestion the caller could not have run.
fn assert_suggestion_runs(fx: &Fixture, argv: &[String]) {
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    let out = fx.run(&args);
    match out.status.code() {
        Some(0) => {}
        Some(3) => {
            let mut json_args = args.clone();
            json_args.push("--json");
            let answer = fx.run(&json_args);
            let value: serde_json::Value = serde_json::from_str(&stdout_of(&answer))
                .expect("a zero-hit answer is still a JSON object");
            assert_eq!(
                value["outcome"], "zero-hit",
                "{args:?} exited 3 for a reason other than an empty answer: {answer:?}"
            );
        }
        other => panic!("{args:?} exited {other:?}: {out:?}"),
    }
}

#[test]
fn refs_and_read_state_the_inbound_total_against_the_rows_they_printed() {
    let fx = Fixture::plain("inbound-count");
    let answer = fx.json(&["refs", "IPriceSource"]);
    let inbound = answer["inbound"].as_object().expect("the five tables");
    let total: u64 = inbound.values().map(|t| t["total"].as_u64().unwrap()).sum();
    let shown: u64 = inbound
        .values()
        .map(|t| t["rows"].as_array().unwrap().len() as u64)
        .sum();
    assert!(
        total > 30,
        "the fixture must overflow the inbound cap: {total}"
    );
    assert!(shown < total, "the cap must hide rows: {shown} of {total}");

    let want =
        format!("Toolworks.Pricing.IPriceSource  (interface)  {total} inbound (showing {shown})");
    for args in [
        vec!["refs", "IPriceSource"],
        vec!["refs", "IPriceSource", "--compact"],
        vec!["read", "IPriceSource"],
        vec!["read", "IPriceSource", "--compact"],
    ] {
        assert_eq!(first_line(&fx.stdout(&args)), want, "{args:?}");
    }
}

#[test]
fn impact_states_the_files_it_reached_against_the_rows_it_printed() {
    let fx = Fixture::plain("impact-count");
    let answer = fx.json(&["impact", "IPriceSource"]);
    let rows = answer["rows"].as_array().unwrap().len() as u64;
    let dropped = answer["dropped"].as_u64().unwrap();
    assert!(
        rows + dropped > 50,
        "the fixture must overflow the cap: {rows}+{dropped}"
    );
    assert!(rows < rows + dropped, "the cap must hide rows");

    let want = format!("{} files (showing {rows})", rows + dropped);
    for args in [
        vec!["impact", "IPriceSource"],
        vec!["impact", "IPriceSource", "--compact"],
    ] {
        let text = fx.stdout(&args);
        assert!(
            first_line(&text).ends_with(&want),
            "{args:?}: {}",
            first_line(&text)
        );
    }
}

#[test]
fn tests_states_the_rows_it_printed_uncapped() {
    let fx = Fixture::plain("tests-count");
    for args in [
        vec!["tests", "IPriceSource"],
        vec!["tests", "IPriceSource", "--compact"],
    ] {
        let text = fx.stdout(&args);
        assert!(
            first_line(&text).ends_with("1 files (showing 1)"),
            "{args:?}: {}",
            first_line(&text)
        );
    }
}

#[test]
fn find_states_no_count_because_it_prints_no_capped_table() {
    let fx = Fixture::plain("find-count");

    // `find`'s first line is a declaration or manifest row, so the count line
    // belongs to the four table verbs alone.
    let text = fx.stdout(&["find", "PriceSource"]);
    assert!(!first_line(&text).contains("(showing "), "{text}");
}

#[test]
fn find_reads_a_format_flag_as_one_more_query_word() {
    let fx = Fixture::plain("find-formats");

    let named = fx.stdout(&["find", "PriceSource"]);
    assert!(
        last_line(&named).starts_with("next: devscout read "),
        "{named}"
    );

    // `--resources` is `find`'s only flag, so `--compact` joins the query
    // instead of selecting a format: the words name no declaration, and the
    // answer keeps the one text shape `find` has -- no declaration block, so
    // no trailer either.
    let with_flag = fx.stdout(&["find", "PriceSource", "--compact"]);
    assert_ne!(with_flag, named, "{with_flag}");
    assert!(!with_flag.contains("next: "), "{with_flag}");
}

#[test]
fn a_zero_hit_answer_states_zero_rather_than_printing_nothing() {
    let fx = Fixture::plain("zero-count");

    let refs = fx.run(&["refs", "UnusedReport.Total"]);
    assert_eq!(refs.status.code(), Some(3), "{refs:?}");
    assert_eq!(
        first_line(&stdout_of(&refs)),
        "Toolworks.Pricing.UnusedReport.Total  (member)  0 inbound (showing 0)"
    );

    let impact = fx.run(&["impact", "UnusedReport.cs"]);
    assert_eq!(impact.status.code(), Some(3), "{impact:?}");
    assert!(
        first_line(&stdout_of(&impact)).ends_with("0 files (showing 0)"),
        "{impact:?}"
    );

    assert_eq!(
        first_line(&fx.stdout(&["tests", "UnusedReport"])),
        "tests for Toolworks.Pricing.UnusedReport  0 files (showing 0)"
    );
}

#[test]
fn every_verb_ends_its_text_answer_with_the_next_verb() {
    let fx = Fixture::plain("trailer");
    let read_like =
        "next: devscout tests Toolworks.Pricing.IPriceSource | devscout impact IPriceSource.cs";
    let cases: [(&[&str], &str); 9] = [
        (&["refs", "IPriceSource"], read_like),
        (&["refs", "IPriceSource", "--compact"], read_like),
        (&["read", "IPriceSource"], read_like),
        (&["read", "IPriceSource", "--compact"], read_like),
        (
            &["tests", "IPriceSource"],
            "next: devscout impact IPriceSource.cs",
        ),
        (
            &["tests", "IPriceSource", "--compact"],
            "next: devscout impact IPriceSource.cs",
        ),
        (
            &["impact", "IPriceSource"],
            "next: devscout refs Toolworks.Pricing.IPriceSource",
        ),
        (
            &["impact", "IPriceSource", "--compact"],
            "next: devscout refs Toolworks.Pricing.IPriceSource",
        ),
        (
            &["find", "PriceSource"],
            "next: devscout read Toolworks.Pricing.CachePriceSource",
        ),
    ];
    for (args, want) in cases {
        assert_eq!(last_line(&fx.stdout(args)), want, "{args:?}");
    }
}

#[test]
fn the_trailer_is_absent_when_nothing_is_left_to_suggest() {
    let fx = Fixture::plain("no-trailer");

    // A resolved seed that reached no file has no blast radius to follow up on.
    let impact = fx.answer(&["impact", "UnusedReport.cs"]);
    assert!(!impact.contains("next: "), "{impact}");

    // A query the manifest answers and the name index does not names no
    // declaration, so there is nothing to read.
    let find = fx.stdout(&["find", "README"]);
    assert!(!find.contains("next: "), "{find}");
}

#[test]
fn the_trailer_comes_from_the_first_declaration_not_the_first_row() {
    let fx = Fixture::with_mixed_pool("mixed-pool");

    // The markup row and the class row are both printed, markup first, and
    // only the class names a declaration `read` opens.
    let text = fx.stdout(&["find", "Panel"]);
    assert!(
        text.contains("src/AView.xaml:1  <UserControl x:Class=\"Demo.Panel\">"),
        "{text}"
    );
    assert!(
        text.contains("src/ZPanel.cs:3  public class Panel"),
        "{text}"
    );
    assert_eq!(last_line(&text), "next: devscout read Demo.Panel", "{text}");

    // `--resources` puts the resource key in the same pool, ahead of both.
    let with_resources = fx.stdout(&["find", "Panel", "--resources"]);
    assert!(
        with_resources.contains("src/AStrings.resw:3  <data name=\"Panel\""),
        "{with_resources}"
    );
    assert_eq!(
        last_line(&with_resources),
        "next: devscout read Demo.Panel",
        "{with_resources}"
    );
}

#[test]
fn the_trailer_never_names_a_declaration_the_block_dropped() {
    // One resource key per printed row, then the class: at 25 keys the
    // declaration is row 26, so the block prints 25 rows and drops it.
    let hidden = Fixture::with_capped_pool("capped-hidden", 25);
    let text = hidden.stdout(&["find", "Panel", "--resources"]);
    assert!(
        text.contains("… +1 more declarations (refine query)"),
        "{text}"
    );
    // The manifest block still lists the file, so the claim is about the
    // declaration rendering rather than the path appearing anywhere.
    assert!(!text.contains("public class Panel"), "{text}");
    assert!(!text.contains("next: "), "{text}");

    // One key fewer and the declaration is the last row the block printed, so
    // it is the row the trailer names.
    let shown = Fixture::with_capped_pool("capped-shown", 24);
    let text = shown.stdout(&["find", "Panel", "--resources"]);
    assert!(!text.contains("more declarations"), "{text}");
    assert!(
        text.contains("src/ZPanel.cs:3  public class Panel"),
        "{text}"
    );
    assert_eq!(last_line(&text), "next: devscout read Demo.Panel", "{text}");
}

#[test]
fn a_path_with_a_space_is_quoted_so_the_line_still_runs() {
    let fx = Fixture::with_spaced_paths("spaced");
    let text = fx.stdout(&["refs", "ILedger"]);
    assert_eq!(
        last_line(&text),
        "next: devscout tests Toolworks.Ledgers.ILedger | devscout impact 'Spaced Dir/ILedger.cs'"
    );

    let suggested = suggestions_from_text(&text);
    assert_eq!(
        suggested[1],
        vec!["impact".to_string(), "Spaced Dir/ILedger.cs".to_string()]
    );
    assert_suggestion_runs(&fx, &suggested[1]);
}

#[test]
fn every_resolved_answer_carries_its_suggestions_as_json_argvs() {
    let fx = Fixture::plain("json-next");
    let read_like = r#"[["tests","Toolworks.Pricing.IPriceSource"],["impact","IPriceSource.cs"]]"#;
    let cases: [(&[&str], &str); 4] = [
        (&["refs", "IPriceSource"], read_like),
        (&["read", "IPriceSource"], read_like),
        (
            &["tests", "IPriceSource"],
            r#"[["impact","IPriceSource.cs"]]"#,
        ),
        (
            &["impact", "IPriceSource"],
            r#"[["refs","Toolworks.Pricing.IPriceSource"]]"#,
        ),
    ];
    for (args, want) in cases {
        let mut json_args = args.to_vec();
        json_args.push("--json");
        let text = fx.stdout(&json_args);
        assert!(
            text.contains(&format!("\"next\":{want},\"outcome\":")),
            "{args:?}: the key sits immediately before `outcome`: {text}"
        );
        let answer: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(answer["schema_version"], 1, "{args:?}");
    }
}

#[test]
fn a_members_answer_carries_next_on_every_member_and_on_the_wrapper() {
    let fx = Fixture::plain("json-members");
    let text = fx.answer(&["refs", "Total", "--json"]);
    let want = r#"[["tests","Toolworks.Pricing.UnusedReport.Total"],["impact","UnusedReport.cs"]]"#;
    let answer: serde_json::Value = serde_json::from_str(&text).unwrap();

    assert_eq!(answer["status"], "members");
    assert_eq!(answer["next"].to_string().replace(' ', ""), want);
    assert_eq!(
        answer["members"][0]["next"].to_string().replace(' ', ""),
        want
    );
    assert!(
        text.contains(&format!("\"next\":{want},\"outcome\":")),
        "the wrapper's key sits before its own outcome: {text}"
    );
}

#[test]
fn an_answer_that_resolved_nothing_carries_no_next() {
    let fx = Fixture::plain("json-refused");

    // Two types declare the member and nothing references either, so the seed
    // is refused instead of guessed between them.
    let ambiguous = fx.run(&["refs", "Reset"]);
    assert_eq!(ambiguous.status.code(), Some(1), "{ambiguous:?}");
    assert!(!stdout_of(&ambiguous).contains("next"));

    let ambiguous_json = fx.answer(&["refs", "Reset", "--json"]);
    assert!(
        ambiguous_json.contains(r#""outcome":"ambiguous""#),
        "{ambiguous_json}"
    );
    assert!(!ambiguous_json.contains("\"next\""), "{ambiguous_json}");

    let missing = fx.run(&["refs", "NoSuchSymbolAtAll"]);
    assert_eq!(missing.status.code(), Some(3), "{missing:?}");
    assert!(!stdout_of(&missing).contains("next"));

    let missing_json = fx.answer(&["refs", "NoSuchSymbolAtAll", "--json"]);
    assert!(
        missing_json.contains(r#""outcome":"fallback-advised""#),
        "{missing_json}"
    );
    assert!(!missing_json.contains("\"next\""), "{missing_json}");
}

#[test]
fn every_emitted_suggestion_runs_against_the_same_index() {
    let fx = Fixture::plain("runnable");
    let mut checked = 0;
    for verb in ["refs", "read", "tests", "impact"] {
        let args = [verb, "IPriceSource"];
        for argv in suggestions_from_text(&fx.stdout(&args)) {
            assert_suggestion_runs(&fx, &argv);
            checked += 1;
        }
        for argv in suggestion_argv(&fx.json(&args)) {
            assert_suggestion_runs(&fx, &argv);
            checked += 1;
        }
    }
    // `find` carries its suggestions on the text trailer alone, so only that
    // half of the check applies to it.
    for argv in suggestions_from_text(&fx.stdout(&["find", "IPriceSource"])) {
        assert_suggestion_runs(&fx, &argv);
        checked += 1;
    }
    assert!(
        checked >= 9,
        "every verb suggests something on this fixture, text and JSON alike: {checked}"
    );

    // A zero-hit answer suggests too, and its suggestions run as well.
    let zero_hit = fx.run(&["refs", "UnusedReport.Total"]);
    for argv in suggestions_from_text(&stdout_of(&zero_hit)) {
        assert_suggestion_runs(&fx, &argv);
    }
}
