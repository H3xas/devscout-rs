//! Integration tests for Angular template reads: component members as
//! `graph.names` rows, `uses-member` edges from external and inline
//! templates, template locals, the `refs` coverage note, the
//! `template_unsupported` rows, and template edits seen by the next `map`.
//!
//! Fixture: `fixtures/angular-templates` (its README lists every form).
//!
//! Every test runs the compiled binary as a subprocess against a fresh copy of
//! the fixture, with `HOME`, `SCOUT_REGISTRY` and `SCOUT_CONTENT_DB` pointed at a
//! fresh temp dir, the same isolation rule the other `cli_*` tests follow.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static COUNTER: AtomicU64 = AtomicU64::new(0);

const CARD_TS: &str = "src/app/profile-card.component.ts";
const CARD_HTML: &str = "src/app/profile-card.component.html";
const MOTTO_TS: &str = "src/app/motto-banner.component.ts";
const MOTTO_HTML: &str = "src/app/motto-banner.component.html";
const BADGE_TS: &str = "src/app/widgets/badge-widget.component.ts";
const BADGE_HTML: &str = "src/app/shared/badge-view.html";
const INLINE_TS: &str = "src/app/inline-notes.component.ts";
const SEARCH_TS: &str = "src/app/search-list.component.ts";
const SEARCH_HTML: &str = "src/app/search-list.component.html";
const ALPHA_HTML: &str = "src/app/panels/alpha-panel.component.html";
const BETA_HTML: &str = "src/app/panels/beta-panel.component.html";
const GAP_HTML: &str = "src/app/gap-showcase.component.html";
const KINDS_TS: &str = "src/app/member-kinds.component.ts";
const SOURCES_TS: &str = "src/app/template-sources.component.ts";
const ORPHAN_HTML: &str = "src/app/orphan-view.html";

fn temp_dir(prefix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-angular-templates-{prefix}-{}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create dest dir");
    for entry in fs::read_dir(src).expect("read source dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name();
        let file_type = entry.file_type().expect("entry file type");
        if file_type.is_dir() {
            copy_tree(&entry.path(), &dst.join(&name));
        } else if file_type.is_file() && name != "README.md" {
            fs::copy(entry.path(), dst.join(&name)).expect("copy fixture file");
        }
    }
}

struct Fixture {
    base: PathBuf,
    repo: PathBuf,
    home: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

impl Fixture {
    fn build(prefix: &str) -> Fixture {
        Fixture::of("angular-templates", prefix)
    }

    fn of(fixture: &str, prefix: &str) -> Fixture {
        let base = temp_dir(prefix);
        let repo = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&home).expect("create home dir");
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures")
                .join(fixture),
            &repo,
        );
        let fx = Fixture { base, repo, home };
        fx.expect_ok(&["init", "--no-hooks", "--no-map"]);
        fx.expect_ok(&["map", "."]);
        fx
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .args(args)
            .current_dir(&self.repo)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .output()
            .expect("devscout must run")
    }

    fn stdout(&self, args: &[&str]) -> String {
        String::from_utf8(self.run(args).stdout).expect("stdout is utf-8")
    }

    fn expect_ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "devscout {args:?} failed: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("stdout is utf-8")
    }

    fn graph(&self) -> Value {
        let text =
            fs::read_to_string(self.repo.join(".scout/graph/graph.json")).expect("read graph.json");
        serde_json::from_str(&text).expect("graph.json is valid JSON")
    }

    fn refs_json(&self, seed: &str) -> Value {
        serde_json::from_str(&self.stdout(&["refs", seed, "--json"])).expect("refs --json")
    }
}

fn owner(file: &str, class: &str) -> String {
    format!("{file}#{class}")
}

/// `(from_file, from_line, to, member)` of every `uses-member` edge.
fn member_edges(graph: &Value) -> Vec<(String, u64, String, String)> {
    graph["edges"]
        .as_array()
        .expect("edges array")
        .iter()
        .filter(|e| e["kind"] == "uses-member")
        .map(|e| {
            assert!(
                e.get("heuristic").is_none(),
                "template edges are precise: {e}"
            );
            (
                e["from_file"].as_str().unwrap().to_string(),
                e["from_line"].as_u64().unwrap(),
                e["to"].as_str().unwrap().to_string(),
                e["member"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn edges_of(graph: &Value, to: &str, member: &str) -> Vec<(String, u64)> {
    member_edges(graph)
        .into_iter()
        .filter(|(_, _, t, m)| t == to && m == member)
        .map(|(f, l, _, _)| (f, l))
        .collect()
}

/// `(file, line)` of every inbound `uses-member` row of a member answer.
fn rows(answer: &Value) -> Vec<(String, u64)> {
    answer["members"][0]["inbound"]["uses-member"]["rows"]
        .as_array()
        .expect("uses-member rows")
        .iter()
        .map(|r| {
            (
                r["file"].as_str().unwrap().to_string(),
                r["line"].as_u64().unwrap(),
            )
        })
        .collect()
}

fn site(file: &str, line: u64) -> (String, u64) {
    (file.to_string(), line)
}

#[test]
fn angular_templates_ac_1_component_members_become_owned_name_rows() {
    let fx = Fixture::build("ac1-rows");
    let graph = fx.graph();
    let names: Vec<(String, String, String, u64, String)> = graph["names"]
        .as_array()
        .expect("names")
        .iter()
        .map(|n| {
            (
                n["name"].as_str().unwrap().to_string(),
                n["kind"].as_str().unwrap().to_string(),
                n["file"].as_str().unwrap().to_string(),
                n["line"].as_u64().unwrap(),
                n["owner"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    let kinds = owner(KINDS_TS, "MemberKinds");
    let of_kinds: Vec<(&str, &str, u64)> = names
        .iter()
        .filter(|n| n.4 == kinds)
        .map(|n| (n.0.as_str(), n.1.as_str(), n.3))
        .collect();
    assert_eq!(
        of_kinds,
        [
            ("expanded", "property", 17),
            ("toggle", "method", 23),
            ("tracker", "field", 26),
            ("ownerName", "field", 27),
        ],
        "a static member and a plain constructor parameter get no row"
    );
    assert!(names
        .iter()
        .all(|n| n.0 != "helperValue" && n.0 != "inheritedLabel"));
    assert!(names.iter().any(|n| n.0 == "caption"
        && n.2 == CARD_TS
        && n.3 == 11
        && n.4 == owner(CARD_TS, "ProfileCard")));
}

#[test]
fn angular_templates_ac_1_bare_and_qualified_seeds_answer_the_declaring_line() {
    let fx = Fixture::build("ac1-seeds");
    for seed in ["caption", "ProfileCard.caption"] {
        let answer = fx.refs_json(seed);
        assert_eq!(
            answer["members"][0]["id"],
            format!("{CARD_TS}#ProfileCard.caption"),
            "{seed}"
        );
        assert_eq!(answer["members"][0]["sites"][0]["line"], 11, "{seed}");
    }
    assert_eq!(fx.refs_json("toggle")["members"][0]["sites"][0]["line"], 23);
    let plain = fx.run(&["refs", "helperValue", "--json"]);
    let plain: Value = serde_json::from_slice(&plain.stdout).unwrap();
    assert_eq!(plain["outcome"], "fallback-advised");
    let other = fx.refs_json("MottoBanner.caption");
    assert_eq!(
        other["outcome"], "fallback-advised",
        "a qualifier narrows to its own class"
    );
}

#[test]
fn angular_templates_ac_2_each_external_form_gives_one_edge() {
    let fx = Fixture::build("ac2-edges");
    let graph = fx.graph();
    let card = owner(CARD_TS, "ProfileCard");
    for (member, line) in [
        ("shown", 2),
        ("rows", 4),
        ("rowKey", 5),
        ("tone", 7),
        ("caption", 8),
        ("label", 9),
        ("selected", 10),
        ("barWidth", 11),
        ("save", 12),
        ("draft", 13),
        ("greeting", 14),
        ("altText", 15),
        ("account", 16),
        ("heading", 17),
        ("currencyCode", 18),
        ("remove", 19),
        ("target", 19),
    ] {
        assert_eq!(
            edges_of(&graph, &card, member),
            [site(CARD_HTML, line)],
            "{member}"
        );
    }
    let to_card: Vec<_> = member_edges(&graph)
        .into_iter()
        .filter(|e| e.2 == card)
        .collect();
    assert_eq!(
        to_card.len(),
        17,
        "a pipe name, a chain tail and a local give no edge"
    );
    assert!(to_card.iter().all(|e| e.0 == CARD_HTML));
    let edge = graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["member"] == "caption")
        .unwrap();
    assert_eq!(edge["to_file"], CARD_TS);
}

#[test]
fn angular_templates_ac_2_refs_lists_the_template_line_for_each_path_form() {
    let fx = Fixture::build("ac2-refs");
    assert_eq!(rows(&fx.refs_json("caption")), [site(CARD_HTML, 8)]);
    assert_eq!(rows(&fx.refs_json("motto")), [site(MOTTO_HTML, 2)]);
    assert_eq!(rows(&fx.refs_json("badgeText")), [site(BADGE_HTML, 2)]);
    let graph = fx.graph();
    assert_eq!(
        edges_of(&graph, &owner(MOTTO_TS, "MottoBanner"), "motto"),
        [site(MOTTO_HTML, 2)]
    );
    assert_eq!(
        edges_of(&graph, &owner(BADGE_TS, "BadgeWidget"), "badgeText"),
        [site(BADGE_HTML, 2)]
    );
    let text = fx.stdout(&["refs", "caption"]);
    assert!(text.contains(&format!("{CARD_HTML}:8")), "{text}");
}

#[test]
fn angular_templates_ac_2_two_reads_on_one_line_answer_each_member_once() {
    let fx = Fixture::build("ac2-same-line");
    assert_eq!(rows(&fx.refs_json("remove")), [site(CARD_HTML, 19)]);
    assert_eq!(rows(&fx.refs_json("target")), [site(CARD_HTML, 19)]);
}

#[test]
fn angular_templates_ac_3_inline_templates_give_edges_from_the_component_file() {
    let fx = Fixture::build("ac3");
    let graph = fx.graph();
    for (class, member, line) in [
        ("SingleNote", "firstNote", 5),
        ("DoubleNote", "secondNote", 13),
        ("BacktickNote", "thirdNote", 23),
        ("BacktickNote", "dismiss", 24),
    ] {
        assert_eq!(
            edges_of(&graph, &owner(INLINE_TS, class), member),
            [site(INLINE_TS, line)],
            "{member}"
        );
    }
    assert_eq!(rows(&fx.refs_json("thirdNote")), [site(INLINE_TS, 23)]);
    assert_eq!(rows(&fx.refs_json("expanded")), [site(KINDS_TS, 8)]);
    assert_eq!(rows(&fx.refs_json("ownerName")), [site(KINDS_TS, 10)]);
}

#[test]
fn angular_templates_ac_4_locals_hide_members_in_their_scope_only() {
    let fx = Fixture::build("ac4");
    let graph = fx.graph();
    let search = owner(SEARCH_TS, "SearchList");
    let edges: Vec<(String, u64, String)> = member_edges(&graph)
        .into_iter()
        .filter(|e| e.2 == search)
        .map(|e| (e.0, e.1, e.3))
        .collect();
    assert_eq!(
        edges,
        [
            (SEARCH_HTML.to_string(), 4, "items".to_string()),
            (SEARCH_HTML.to_string(), 8, "pending".to_string()),
            (SEARCH_HTML.to_string(), 9, "search".to_string()),
            (SEARCH_HTML.to_string(), 10, "item".to_string()),
        ]
    );
    assert_eq!(rows(&fx.refs_json("item")), [site(SEARCH_HTML, 10)]);
    for hidden in ["entry", "position", "job", "searchBox"] {
        assert_eq!(rows(&fx.refs_json(hidden)), [], "{hidden}");
    }
}

#[test]
fn angular_templates_ac_5_an_unread_field_answers_its_declaration_and_no_row() {
    let fx = Fixture::build("ac5-unread");
    let out = fx.run(&["refs", "unusedFlag", "--json"]);
    assert_eq!(out.status.code(), Some(3));
    let answer: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(answer["members"][0]["sites"][0]["line"], 22);
    assert_eq!(rows(&answer), []);
    assert_eq!(answer["outcome"], "zero-hit");
}

#[test]
fn angular_templates_ac_5_a_shared_name_stays_with_its_own_component() {
    let fx = Fixture::build("ac5-shared");
    let out = fx.run(&["refs", "status"]);
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        text.contains("ambiguous member \"status\" — 2 candidates"),
        "{text}"
    );
    assert!(text.contains("#AlphaPanel.status") && text.contains("#BetaPanel.status"));
    assert!(!text.contains("GammaPanel"), "{text}");
    assert_eq!(
        rows(&fx.refs_json("AlphaPanel.status")),
        [site(ALPHA_HTML, 2)]
    );
    assert_eq!(
        rows(&fx.refs_json("BetaPanel.status")),
        [site(BETA_HTML, 3)]
    );
    assert_eq!(rows(&fx.refs_json("GammaPanel.status")), []);
}

#[test]
fn angular_templates_ac_5_a_template_no_component_names_adds_nothing() {
    let fx = Fixture::build("ac5-orphan");
    let graph = fx.graph();
    let named = |v: &Value| v["from_file"] == ORPHAN_HTML || v["file"] == ORPHAN_HTML;
    assert!(!graph["edges"].as_array().unwrap().iter().any(named));
    assert!(!graph["template_unsupported"]
        .as_array()
        .unwrap()
        .iter()
        .any(named));
}

#[test]
fn angular_templates_ac_6_a_typescript_member_answer_carries_the_note() {
    let fx = Fixture::build("ac6-note");
    let note =
        "note: rows are Angular template reads only. TypeScript member reads are not indexed.";
    for args in [
        &["refs", "caption"][..],
        &["refs", "caption", "--compact"][..],
        &["refs", "unusedFlag"][..],
        &["read", "caption"][..],
    ] {
        let text = fx.stdout(args);
        assert_eq!(text.matches(note).count(), 1, "{args:?}: {text}");
        let before_next = text.lines().rev().nth(1).unwrap_or_default();
        assert_eq!(before_next, note, "{args:?}: {text}");
    }
    for seed in ["caption", "unusedFlag"] {
        let answer = fx.refs_json(seed);
        let member = answer["members"][0].as_object().unwrap();
        assert_eq!(member["memberReads"], "angular-templates-only");
        let keys: Vec<&String> = member.keys().collect();
        let at = keys.iter().position(|k| *k == "memberReads").unwrap();
        assert_eq!(keys[at + 1], "next");
        assert!(answer.get("memberReads").is_none());
    }
    let class = fx.stdout(&["refs", "ProfileCard"]);
    assert!(!class.contains("note:"), "{class}");
    assert!(!fx
        .stdout(&["refs", "ProfileCard", "--json"])
        .contains("memberReads"));
    assert!(!fx.stdout(&["refs", "status"]).contains("note:"));
}

#[test]
fn angular_templates_ac_6_csharp_member_answers_keep_their_shape() {
    let fx = Fixture::of("member-seeds", "ac6-csharp");
    let graph = fx.graph();
    let seed = graph["names"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| matches!(n["kind"].as_str(), Some("method" | "property")))
        .and_then(|n| n["name"].as_str())
        .expect("a C# member row")
        .to_string();
    let text = fx.stdout(&["refs", &seed]);
    assert!(!text.contains("Angular"), "{text}");
    assert!(!fx
        .stdout(&["refs", &seed, "--json"])
        .contains("memberReads"));
}

#[test]
fn angular_templates_ac_7_unsupported_rows_carry_each_reason_in_order() {
    let fx = Fixture::build("ac7-rows");
    let graph = fx.graph();
    let found: Vec<(String, u64, String)> = graph["template_unsupported"]
        .as_array()
        .expect("template_unsupported")
        .iter()
        .map(|r| {
            (
                r["file"].as_str().unwrap().to_string(),
                r["line"].as_u64().unwrap(),
                r["reason"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let expected = [
        (GAP_HTML, 1, "control-flow-block"),
        (GAP_HTML, 4, "unparsed-expression"),
        (GAP_HTML, 5, "unknown-member"),
        (KINDS_TS, 11, "unknown-member"),
        (SOURCES_TS, 8, "template-url-unresolved"),
        (SOURCES_TS, 16, "template-url-non-literal"),
        (SOURCES_TS, 24, "inline-template-substitution"),
    ]
    .map(|(f, l, r)| (f.to_string(), l, r.to_string()));
    assert_eq!(found, expected);
    assert_eq!(graph["stats"]["ts"]["template_unsupported_count"], 7);
    let keys: Vec<&String> = graph.as_object().unwrap().keys().collect();
    assert_eq!(keys.last().unwrap().as_str(), "template_unsupported");
    assert_eq!(
        edges_of(
            &graph,
            &owner("src/app/gap-showcase.component.ts", "GapShowcase"),
            "insideBlock"
        ),
        [site(GAP_HTML, 2)],
        "markup inside a control flow block is still scanned"
    );
}

#[test]
fn angular_templates_ac_7_a_repository_without_a_component_has_neither_key() {
    let fx = Fixture::of("ts-query-edges", "ac7-none");
    let graph = fx.graph();
    assert!(graph.get("template_unsupported").is_none());
    assert!(graph["stats"]["ts"]
        .get("template_unsupported_count")
        .is_none());
}

#[test]
fn angular_templates_ac_8_a_template_edit_is_seen_by_the_next_map() {
    let fx = Fixture::build("ac8-edit");
    let unchanged = fx.expect_ok(&["map", "."]);
    assert!(unchanged.contains("graph unchanged"), "{unchanged}");
    assert_eq!(rows(&fx.refs_json("motto")), [site(MOTTO_HTML, 2)]);

    let path = fx.repo.join(MOTTO_HTML);
    fs::write(
        &path,
        "<header>\n  <q>static</q>\n</header>\n<p>{{ motto }}</p>\n",
    )
    .unwrap();
    let rebuilt = fx.expect_ok(&["map", "."]);
    assert!(rebuilt.contains("graph rebuilt"), "{rebuilt}");
    assert_eq!(rows(&fx.refs_json("motto")), [site(MOTTO_HTML, 4)]);
    let again = fx.expect_ok(&["map", "."]);
    assert!(again.contains("graph unchanged"), "{again}");
}

#[test]
fn angular_templates_ac_8_an_earlier_cache_generation_is_not_read() {
    let fx = Fixture::build("ac8-generation");
    let graph_dir = fx.repo.join(".scout/graph");
    let current = graph_dir.join("fragments-v44.json");
    let cached = fs::read_to_string(&current).expect("current cache");
    assert!(
        cached.contains("\"html\":1"),
        "templates are cached as their own shape"
    );
    let index = fs::read_to_string(graph_dir.join("fragments-index-v44.json")).unwrap();
    fs::write(graph_dir.join("fragments-v41.json"), &cached).unwrap();
    fs::write(graph_dir.join("fragments-index-v41.json"), &index).unwrap();
    fs::remove_file(&current).unwrap();
    fs::remove_file(graph_dir.join("fragments-index-v44.json")).unwrap();

    let out = fx.expect_ok(&["map", "."]);
    assert!(out.contains("graph rebuilt"), "{out}");
    assert!(current.exists());
    assert!(!graph_dir.join("fragments-v41.json").exists());
    assert!(!graph_dir.join("fragments-index-v41.json").exists());
    assert_eq!(rows(&fx.refs_json("caption")), [site(CARD_HTML, 8)]);
}

#[test]
fn angular_templates_ac_8_a_deleted_template_becomes_an_unresolved_row() {
    let fx = Fixture::build("ac8-deleted");
    fs::remove_file(fx.repo.join(MOTTO_HTML)).unwrap();
    let rebuilt = fx.expect_ok(&["map", "."]);
    assert!(rebuilt.contains("graph rebuilt"), "{rebuilt}");
    assert_eq!(rows(&fx.refs_json("motto")), []);
    let graph = fx.graph();
    assert!(graph["template_unsupported"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["file"] == MOTTO_TS
            && r["line"] == 5
            && r["reason"] == "template-url-unresolved"));
}
