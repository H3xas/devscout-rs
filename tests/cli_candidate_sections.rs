//! Integration coverage for `fixtures/candidate-sections/`: what `--pick`
//! answers on `refs`/`read`/`impact`/`tests` when a seed names more than one
//! candidate (see the fixture's own README for the case table).
//!
//! Every test here goes through the COMPILED BINARY as a subprocess against a
//! graph the binary itself mapped, the same device `cli_member_seeds.rs` uses:
//! resolution reads real source text off disk, so an in-process model call
//! could not exercise the same path.
//!
//! The load-bearing comparison is `section <i> == --pick <i>`: both are run
//! here and compared byte for byte, which is what makes "a section is that
//! candidate's own answer" a property of the built binary rather than of a
//! reading of a diff.
//!
//! SAFETY: every spawned process gets HOME, SCOUT_REGISTRY and
//! SCOUT_CONTENT_DB pointed at a fresh temp dir, so the operator's real
//! settings and registry are never read and never written.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use devscout_rs::usage;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-candidate-sections-{prefix}-{}-{n}",
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

const FIXTURE_FILES: &[&str] = &[
    "IHold.cs",
    "Hold.cs",
    "Crane.cs",
    "Dock.cs",
    "Manifest.cs",
    "Office.cs",
    "Purser.cs",
    "Clerk.cs",
    "Logbook.cs",
    "Galley.cs",
    "FleetGalley.cs",
    "Cook.cs",
    "Mooring.cs",
    "Hawser.cs",
];

struct Fixture {
    root: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn build() -> Fixture {
        let base = temp_dir("fixture");
        let root = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&root).expect("create repo dir");
        fs::create_dir_all(&home).expect("create home dir");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/candidate-sections");
        for name in FIXTURE_FILES {
            fs::copy(source.join(name), root.join(name)).expect("copy fixture file");
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
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("stderr is utf-8")
}

// The seeds every test below reads. The member seed names an interface member
// and its implementation; the type seed names one type in two namespaces.
const MEMBER_SEED: &str = "Stow";
const TYPE_SEED: &str = "Manifest";
const SUFFIX_PAIR_SEED: &str = "Simmer";
const ZERO_HIT_SEED: &str = "Furl";
const VERBS: [&str; 4] = ["refs", "read", "impact", "tests"];

const MEMBER_REFUSAL: &str = "ambiguous member \"Stow\" — 2 candidates:\n\
                               Nautical.Crew.Hold.Stow  Hold.cs:5\n\
                               Nautical.Crew.IHold.Stow  IHold.cs:5\n";

const TYPE_REFUSAL: &str = "ambiguous symbol \"Manifest\" — 2 candidates:\n\
                            Harbor.Office.Manifest  Office.cs:3  class\n\
                            Nautical.Crew.Manifest  Manifest.cs:3  class\n";

fn usage_line_of(verb: &str) -> &'static str {
    match verb {
        "refs" => usage::REFS_USAGE,
        "read" => usage::READ_USAGE,
        "impact" => usage::IMPACT_USAGE,
        _ => usage::TESTS_USAGE,
    }
}

// One candidate's own answer, as the verb prints it for `--pick <n>`. A
// section must equal this string exactly.
fn pick_stdout(fx: &Fixture, verb: &str, seed: &str, n: usize, extra: &[&str]) -> String {
    let n = n.to_string();
    let mut args = vec![verb, seed, "--pick", &n];
    args.extend_from_slice(extra);
    let out = fx.run(&args);
    assert!(
        matches!(out.status.code(), Some(0) | Some(3)),
        "{args:?} must answer one candidate: {out:?}"
    );
    let stdout = stdout_of(&out);
    stdout.strip_suffix('\n').unwrap_or(&stdout).to_string()
}

// The `--pick all` answer split into its label line and its body, one entry per
// candidate. A section body may itself hold a blank line, so the split is on
// the label lines and never on blank lines; the one blank line that separates
// two sections is asserted where two labels meet.
fn sections_of(stdout: &str, n: usize) -> Vec<(String, String)> {
    let stdout = stdout.strip_suffix('\n').unwrap_or(stdout);
    let lines: Vec<&str> = stdout.split('\n').collect();
    let labels: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| is_section_label(line))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(labels.len(), n, "one label line per candidate: {stdout}");
    let mut sections = Vec::new();
    for (i, &at) in labels.iter().enumerate() {
        let end = match labels.get(i + 1) {
            Some(&next) => {
                assert_eq!(
                    lines[next - 1],
                    "",
                    "one blank line separates two sections: {stdout}"
                );
                next - 1
            }
            None => lines.len(),
        };
        sections.push((lines[at].to_string(), lines[at + 1..end].join("\n")));
    }
    sections
}

// `candidate <i> of <n>: <row>` -- the label line, and nothing a body is
// likely to start with by accident.
fn is_section_label(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("candidate ") else {
        return false;
    };
    let Some((i, rest)) = rest.split_once(" of ") else {
        return false;
    };
    let Some((n, _)) = rest.split_once(": ") else {
        return false;
    };
    !i.is_empty()
        && !n.is_empty()
        && i.chars().all(|c| c.is_ascii_digit())
        && n.chars().all(|c| c.is_ascii_digit())
}

#[test]
fn candidate_sections_ac_1_the_default_refusal_is_unchanged_on_every_verb() {
    let fx = Fixture::build();

    for verb in VERBS {
        let member = fx.run(&[verb, MEMBER_SEED]);
        assert_eq!(member.status.code(), Some(1), "{verb}: {member:?}");
        assert_eq!(stdout_of(&member), MEMBER_REFUSAL, "{verb}");

        let typed = fx.run(&[verb, TYPE_SEED]);
        assert_eq!(typed.status.code(), Some(1), "{verb}: {typed:?}");
        assert_eq!(stdout_of(&typed), TYPE_REFUSAL, "{verb}");

        for out in [&member, &typed] {
            assert_eq!(stderr_of(out), "", "{verb}: a refusal is not a zero hit");
        }
    }
}

#[test]
fn candidate_sections_ac_1_the_refusal_json_lists_the_same_candidates_in_the_same_order() {
    let fx = Fixture::build();

    let member = r#"{"schema_version":1,"outcome":"ambiguous","query":"Stow","candidates":[{"owner":"Nautical.Crew.Hold","name":"Stow","file":"Hold.cs","line":5},{"owner":"Nautical.Crew.IHold","name":"Stow","file":"IHold.cs","line":5}],"freshness":{"state":"fresh"}}"#;
    let typed = r#"{"schema_version":1,"outcome":"ambiguous","query":"Manifest","candidates":[{"id":"Harbor.Office.Manifest","file":"Office.cs","line":3,"kind":"class"},{"id":"Nautical.Crew.Manifest","file":"Manifest.cs","line":3,"kind":"class"}],"freshness":{"state":"fresh"}}"#;

    for verb in VERBS {
        let member_out = fx.run(&[verb, MEMBER_SEED, "--json"]);
        assert_eq!(member_out.status.code(), Some(1), "{verb}: {member_out:?}");
        assert_eq!(stdout_of(&member_out).trim_end(), member, "{verb}");

        let typed_out = fx.run(&[verb, TYPE_SEED, "--json"]);
        assert_eq!(typed_out.status.code(), Some(1), "{verb}: {typed_out:?}");
        assert_eq!(stdout_of(&typed_out).trim_end(), typed, "{verb}");
    }
}

#[test]
fn candidate_sections_ac_2_every_other_flag_leaves_each_section_that_candidates_pick_answer() {
    let fx = Fixture::build();

    // `--pick all` is decided before the answer is built, so every flag the
    // verb already took reaches each candidate the same way it reaches a
    // single-seed run.
    for (verb, extra) in [
        ("refs", vec!["--out"]),
        ("refs", vec!["--all"]),
        ("refs", vec!["--out", "--all"]),
        ("read", vec!["--no-guess"]),
        ("tests", vec!["--no-dispatch"]),
        ("impact", vec!["--hops", "3"]),
        ("impact", vec!["--no-iface"]),
        ("impact", vec!["--iface-max-fanin", "0"]),
        ("impact", vec!["--hub-max-indegree", "0"]),
    ] {
        for seed in [MEMBER_SEED, TYPE_SEED] {
            let mut args = vec![verb, seed, "--pick", "all"];
            args.extend_from_slice(&extra);
            let all = fx.run(&args);
            assert!(all.status.success(), "{args:?}: {all:?}");

            let sections = sections_of(&stdout_of(&all), 2);
            for (i, (_, body)) in sections.iter().enumerate() {
                assert_eq!(
                    body,
                    &pick_stdout(&fx, verb, seed, i + 1, &extra),
                    "{args:?}: section {} is that candidate's own answer",
                    i + 1
                );
            }
        }
    }
}

#[test]
fn candidate_sections_ac_2_the_first_pick_flag_counts() {
    let fx = Fixture::build();

    let first_nth = fx.run(&["refs", MEMBER_SEED, "--pick", "1", "--pick", "all"]);
    assert_eq!(
        stdout_of(&first_nth),
        stdout_of(&fx.run(&["refs", MEMBER_SEED, "--pick", "1"])),
        "a repeated `--pick` is not a second narrowing: the first one counts"
    );

    let first_all = fx.run(&["refs", MEMBER_SEED, "--pick", "all", "--pick", "1"]);
    assert!(
        stdout_of(&first_all).starts_with("candidate 1 of 2: "),
        "{first_all:?}"
    );
}

#[test]
fn candidate_sections_ac_2_each_section_is_that_candidates_pick_answer() {
    let fx = Fixture::build();
    for (seed, rows) in [
        (
            MEMBER_SEED,
            [
                "Nautical.Crew.Hold.Stow  Hold.cs:5",
                "Nautical.Crew.IHold.Stow  IHold.cs:5",
            ],
        ),
        (
            TYPE_SEED,
            [
                "Harbor.Office.Manifest  Office.cs:3  class",
                "Nautical.Crew.Manifest  Manifest.cs:3  class",
            ],
        ),
    ] {
        for verb in VERBS {
            for extra in [vec![], vec!["--compact"]] {
                let mut args = vec![verb, seed, "--pick", "all"];
                args.extend_from_slice(&extra);
                let all = fx.run(&args);
                assert_eq!(all.status.code(), Some(0), "{args:?}: {all:?}");

                let sections = sections_of(&stdout_of(&all), 2);
                for (i, (label, body)) in sections.iter().enumerate() {
                    assert_eq!(
                        label,
                        &format!("candidate {} of 2: {}", i + 1, rows[i]),
                        "{args:?}: the label quotes the refusal's own row"
                    );
                    assert_eq!(
                        body,
                        &pick_stdout(&fx, verb, seed, i + 1, &extra),
                        "{args:?}: section {} must be that candidate's own answer",
                        i + 1
                    );
                }
                assert!(
                    sections[0].1 != sections[1].1,
                    "{args:?}: two candidates are never one merged answer"
                );
            }
        }
    }
}

#[test]
fn candidate_sections_ac_3_the_sections_wrapper_carries_one_object_per_candidate() {
    let fx = Fixture::build();

    for seed in [MEMBER_SEED, TYPE_SEED] {
        // The refusal's own candidate objects, in its own order.
        let refused = fx.run(&[VERBS[0], seed, "--json"]);
        let refused: serde_json::Value =
            serde_json::from_str(stdout_of(&refused).trim_end()).expect("refusal json");
        let candidates = refused["candidates"]
            .as_array()
            .expect("candidates")
            .clone();
        assert_eq!(candidates.len(), 2);

        for verb in VERBS {
            let all = fx.run(&[verb, seed, "--pick", "all", "--json"]);
            assert_eq!(all.status.code(), Some(0), "{verb}: {all:?}");
            let raw = stdout_of(&all);
            let raw = raw.trim_end();

            // Key order is read from the bytes, not from a parsed map, because
            // a map would answer a different question.
            let order = [
                "\"schema_version\"",
                "\"status\"",
                "\"query\"",
                "\"sections\"",
                "\"next\"",
                "\"outcome\"",
                "\"freshness\"",
            ];
            let mut at = 0;
            for key in order {
                let found = raw[at..]
                    .find(key)
                    .unwrap_or_else(|| panic!("{verb}: {key} missing after byte {at}: {raw}"));
                at += found + key.len();
            }

            let value: serde_json::Value = serde_json::from_str(raw).expect("sections json");
            assert_eq!(value["schema_version"], 1, "{verb}");
            assert_eq!(value["status"], "sections", "{verb}");
            assert_eq!(value["query"], seed, "{verb}");
            assert_eq!(value["outcome"], "hit", "{verb}");

            let sections = value["sections"].as_array().expect("sections").clone();
            assert_eq!(sections.len(), 2, "{verb}");
            for (i, section) in sections.iter().enumerate() {
                assert_eq!(section["pick"], i + 1, "{verb}");
                assert_eq!(
                    section["candidate"], candidates[i],
                    "{verb}: the section names the refusal's own candidate object"
                );

                // Each answer is the `--pick <i> --json` object without its own
                // `freshness`, which the wrapper carries once at the end.
                let alone = fx.run(&[verb, seed, "--pick", &(i + 1).to_string(), "--json"]);
                let alone: serde_json::Value =
                    serde_json::from_str(stdout_of(&alone).trim_end()).expect("pick json");
                let mut expected = alone.clone();
                expected
                    .as_object_mut()
                    .expect("object")
                    .remove("freshness");
                assert_eq!(section["answer"], expected, "{verb}: section {}", i + 1);
            }

            // `next` is the union of the sections' own lists.
            let union: Vec<serde_json::Value> = sections
                .iter()
                .flat_map(|s| {
                    s["answer"]["next"]
                        .as_array()
                        .cloned()
                        .unwrap_or_else(|| panic!("{verb}: every section states its own next"))
                })
                .collect();
            assert_eq!(
                value["next"].as_array().expect("next"),
                &union,
                "{verb}: the wrapper states the sections' own union"
            );
        }
    }
}

#[test]
fn candidate_sections_ac_3_a_wrapper_of_empty_sections_is_a_zero_hit() {
    let fx = Fixture::build();

    for verb in ["refs", "impact"] {
        let all = fx.run(&[verb, ZERO_HIT_SEED, "--pick", "all", "--json"]);
        assert_eq!(all.status.code(), Some(3), "{verb}: {all:?}");
        let value: serde_json::Value =
            serde_json::from_str(stdout_of(&all).trim_end()).expect("sections json");
        assert_eq!(value["status"], "sections", "{verb}");
        assert_eq!(value["outcome"], "zero-hit", "{verb}");
        for section in value["sections"].as_array().expect("sections") {
            assert_eq!(section["answer"]["outcome"], "zero-hit", "{verb}");
        }

        // `refs` declines the text-search note for a resolved-but-empty
        // answer; `impact` states its own note once, on the wrapper.
        let note = stderr_of(&all);
        if verb == "refs" {
            assert_eq!(note, "", "{verb}");
        } else {
            assert_eq!(
                note.matches("devscout impact: zero hits").count(),
                1,
                "{verb}: one note for the answer as a whole"
            );
        }
    }
}

#[test]
fn candidate_sections_ac_3_a_wrapper_with_one_hit_section_is_a_hit() {
    let fx = Fixture::build();

    // Only the harbour office manifest carries a test, so the two `tests`
    // sections differ while the wrapper stays a hit either way.
    let all = fx.run(&["tests", TYPE_SEED, "--pick", "all", "--json"]);
    assert_eq!(all.status.code(), Some(0), "{all:?}");
    let value: serde_json::Value =
        serde_json::from_str(stdout_of(&all).trim_end()).expect("sections json");
    assert_eq!(value["outcome"], "hit", "{all:?}");
    let sections = value["sections"].as_array().expect("sections");
    assert_eq!(
        sections[0]["answer"]["rows"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        sections[1]["answer"]["rows"].as_array().map(Vec::len),
        Some(0)
    );
}

#[test]
fn candidate_sections_ac_4_a_type_candidate_answers_as_that_def_id_would() {
    let fx = Fixture::build();

    for (n, id) in [
        ("1", "Harbor.Office.Manifest"),
        ("2", "Nautical.Crew.Manifest"),
    ] {
        for verb in ["refs", "read", "tests"] {
            let picked = fx.run(&[verb, TYPE_SEED, "--pick", n]);
            let by_id = fx.run(&[verb, id]);
            assert_eq!(picked.status.code(), by_id.status.code(), "{verb} {n}");
            assert_eq!(
                stdout_of(&picked),
                stdout_of(&by_id),
                "{verb} {n}: the picked type echoes its def id, so the two runs are one answer"
            );
            assert_eq!(stderr_of(&picked), stderr_of(&by_id), "{verb} {n}");
        }
    }
}

#[test]
fn candidate_sections_ac_4_an_out_of_range_pick_is_a_usage_error() {
    let fx = Fixture::build();

    for verb in VERBS {
        let out = fx.run(&[verb, TYPE_SEED, "--pick", "99"]);
        assert_eq!(out.status.code(), Some(2), "{verb}: {out:?}");
        assert_eq!(
            stdout_of(&out),
            format!("{}\n", usage_line_of(verb)),
            "{verb}"
        );
    }
}

#[test]
fn candidate_sections_ac_5_a_suffix_pair_answers_one_owner_each() {
    let fx = Fixture::build();

    // The refusal still lists both owners, and neither `--pick <n>` answers
    // the ambiguity again.
    let refused = fx.run(&["refs", SUFFIX_PAIR_SEED]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");

    for (n, owner, caller, other_caller) in [
        ("1", "Fleet.Nautical.Crew.Galley", "Cook.cs:8", "Cook.cs:7"),
        ("2", "Nautical.Crew.Galley", "Cook.cs:7", "Cook.cs:8"),
    ] {
        for verb in VERBS {
            let picked = fx.run(&[verb, SUFFIX_PAIR_SEED, "--pick", n]);
            assert_ne!(
                picked.status.code(),
                Some(1),
                "{verb} {n}: a chosen owner is never ambiguous again: {picked:?}"
            );
        }
        let text = pick_stdout(
            &fx,
            "refs",
            SUFFIX_PAIR_SEED,
            n.parse().expect("one-based number"),
            &[],
        );
        assert!(text.contains(owner), "{owner}: {text}");
        assert!(
            text.contains(caller),
            "{owner} keeps its own caller: {text}"
        );
        assert!(
            !text.contains(other_caller),
            "{owner} must not carry the other owner's caller: {text}"
        );
    }
}

#[test]
fn candidate_sections_ac_5_each_suffix_pair_section_is_that_candidates_pick_answer() {
    let fx = Fixture::build();

    for verb in VERBS {
        for extra in [vec![], vec!["--compact"]] {
            let mut args = vec![verb, SUFFIX_PAIR_SEED, "--pick", "all"];
            args.extend_from_slice(&extra);
            let all = fx.run(&args);
            assert_ne!(all.status.code(), Some(1), "{args:?}: {all:?}");
            let sections = sections_of(&stdout_of(&all), 2);
            for (i, (_, body)) in sections.iter().enumerate() {
                assert_eq!(
                    body,
                    &pick_stdout(&fx, verb, SUFFIX_PAIR_SEED, i + 1, &extra),
                    "{args:?}: section {} is that candidate's own answer",
                    i + 1
                );
            }
        }
    }
}

#[test]
fn candidate_sections_ac_6_pick_all_on_a_seed_that_is_not_ambiguous_changes_nothing() {
    let fx = Fixture::build();

    for args in [
        vec!["refs", "Dock"],
        vec!["refs", "Nautical.Crew.Dock"],
        vec!["read", "Dock"],
        vec!["impact", "Dock.cs"],
        vec!["impact", "Nautical.Crew.Dock"],
        vec!["tests", "Dock"],
        vec!["refs", "ThereIsNoSuchThing"],
        vec!["read", "ThereIsNoSuchThing"],
        vec!["impact", "ThereIsNoSuchThing"],
        vec!["tests", "ThereIsNoSuchThing"],
    ] {
        let base = fx.run(&args);
        let mut with_all = args.clone();
        with_all.extend_from_slice(&["--pick", "all"]);
        let all = fx.run(&with_all);

        assert_eq!(all.status.code(), base.status.code(), "{args:?}");
        assert_eq!(stdout_of(&all), stdout_of(&base), "{args:?}");
        assert_eq!(stderr_of(&all), stderr_of(&base), "{args:?}");
    }
}

// A value the flag refuses is refused before the root is resolved, so these
// run from a directory with no repository at all.
fn run_bare(home: &Path, bare: &Path, args: &[&str]) -> Output {
    Command::new(rust_bin())
        .args(args)
        .current_dir(bare)
        .env("HOME", home)
        .env("SCOUT_REGISTRY", home.join("repos.json"))
        .env("SCOUT_CONTENT_DB", home.join("content.db"))
        .output()
        .expect("devscout must run")
}

#[test]
fn candidate_sections_ac_7_a_malformed_pick_value_is_a_usage_error_before_the_graph_is_read() {
    let bare = temp_dir("no-graph");
    let home = temp_dir("no-graph-home");

    for value in ["0", "-1", "All", "ALL", "two", "1,2", ""] {
        for verb in VERBS {
            let out = run_bare(&home, &bare, &[verb, "Stow", "--pick", value]);
            assert_eq!(
                out.status.code(),
                Some(2),
                "{verb} --pick {value:?}: {out:?}"
            );
            assert_eq!(
                stdout_of(&out),
                format!("{}\n", usage_line_of(verb)),
                "{verb} --pick {value:?}"
            );
        }
    }

    for verb in VERBS {
        let out = run_bare(&home, &bare, &[verb, "Stow", "--pick"]);
        assert_eq!(out.status.code(), Some(2), "{verb}: {out:?}");
        assert_eq!(
            stdout_of(&out),
            format!("{}\n", usage_line_of(verb)),
            "{verb}"
        );
    }
}
