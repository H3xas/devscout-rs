//! Integration tests for the enriched-lane record: the second `map` stdout
//! line and the `stats.lane` object in `graph.json`, for every way an admitted
//! compiler-facts artifact can fail to apply and for the applied run.
//!
//! See `fixtures/csharp-enrichment/README.md`.

mod support;

use std::fs;

use serde_json::{json, Value};
use support::{candidate, confirmed_load_site, lane_line, sha1_hex, Fixture};

/// Asserts the four things every not-applied run owes: the exact lane line,
/// the exact `stats.lane` record, no `stats.semantic`, and (through
/// `Fixture::map`) exit 0.
fn assert_not_applied(stdout: &str, graph: &Value, line: &str, record: Value) {
    assert_eq!(lane_line(stdout), Some(line), "{stdout}");
    assert_eq!(stdout.lines().count(), 2, "{stdout}");
    assert!(!stdout.contains("lane: enriched"), "{stdout}");
    assert_eq!(graph["stats"]["lane"], record, "{}", graph["stats"]);
    assert!(
        graph["stats"].get("semantic").is_none(),
        "a lane that did not apply writes no semantic counters: {}",
        graph["stats"]
    );
}

/// The edges a second copy of the fixture yields with no artifact at all,
/// after `prepare` applies the same change to it.
fn no_artifact_edges(prefix: &str, prepare: impl Fn(&Fixture)) -> Value {
    let twin = Fixture::build(prefix);
    prepare(&twin);
    twin.map(&[]);
    let g = twin.graph();
    assert!(g["stats"].get("lane").is_none(), "{}", g["stats"]);
    g["edges"].clone()
}

/// Maps the fixture once with no artifact, then admits a fresh artifact that
/// confirms one ambiguous call, and maps again. Returns the applied stdout.
fn admit_fresh(fx: &Fixture, imports: Vec<Value>) -> String {
    fx.map(&[]);
    let line = fx.first_load_line();
    fx.import(&candidate(
        &fx.head(),
        imports,
        vec![confirmed_load_site(line)],
    ));
    fx.map(&[])
}

#[test]
fn a_fresh_admitted_artifact_prints_the_enriched_lane_and_records_applied() {
    let fx = Fixture::build("applied");
    let stdout = admit_fresh(&fx, vec![]);

    assert_eq!(lane_line(&stdout), Some("lane: enriched"), "{stdout}");
    assert_eq!(stdout.lines().count(), 2, "{stdout}");
    assert!(!stdout.contains("not applied"), "{stdout}");
    let g = fx.graph();
    assert_eq!(
        g["stats"]["lane"],
        json!({"state": "applied", "compilations": {"total": 1, "partial": 0}})
    );
    assert_eq!(g["stats"]["semantic"]["confirmed"], 1, "{}", g["stats"]);
    assert_eq!(g["stats"]["semantic"]["disagreements"], 1, "{}", g["stats"]);
    let text = String::from_utf8(fx.graph_bytes()).unwrap();
    let semantic_at = text.find(r#""semantic":{"#).expect("semantic key");
    let lane_at = text
        .find(r#","lane":{"state":"applied","compilations":{"total":1,"partial":0}}},"#)
        .expect("lane closes the stats object");
    assert!(semantic_at < lane_at, "lane is appended after semantic");
    assert_eq!(fx.audit_lane(), "enriched");
}

#[test]
fn a_moved_head_is_reported_as_a_stale_artifact_and_leaves_the_syntax_edges() {
    let fx = Fixture::build("head-moved");
    admit_fresh(&fx, vec![]);
    fx.commit_unrelated_file();
    let stdout = fx.map(&[]);

    let g = fx.graph();
    assert_not_applied(
        &stdout,
        &g,
        "lane: syntax (enriched lane not applied: stale-artifact: source-head-mismatch)",
        json!({"state": "not-applied", "reason": "stale-artifact", "detail": "source-head-mismatch"}),
    );
    assert_eq!(
        g["edges"],
        no_artifact_edges("head-moved-twin", Fixture::commit_unrelated_file)
    );
    assert_eq!(fx.audit_lane(), "syntax");
}

#[test]
fn an_untracked_file_is_reported_as_a_stale_artifact_on_a_dirty_tree() {
    let fx = Fixture::build("dirty-tree");
    admit_fresh(&fx, vec![]);
    let untracked = |f: &Fixture| fs::write(f.repo.join("notes.txt"), "notes\n").unwrap();
    untracked(&fx);
    let stdout = fx.map(&[]);

    let g = fx.graph();
    assert_not_applied(
        &stdout,
        &g,
        "lane: syntax (enriched lane not applied: stale-artifact: working-tree-dirty)",
        json!({"state": "not-applied", "reason": "stale-artifact", "detail": "working-tree-dirty"}),
    );
    assert_eq!(g["edges"], no_artifact_edges("dirty-tree-twin", untracked));
    assert_eq!(fx.audit_lane(), "syntax");
}

#[test]
fn a_second_map_from_the_same_stale_artifact_writes_identical_bytes() {
    let fx = Fixture::build("stale-twice");
    admit_fresh(&fx, vec![]);
    fx.commit_unrelated_file();
    let first_out = fx.map(&[]);
    let first = fx.graph_bytes();
    let second_out = fx.map(&[]);
    assert_eq!(lane_line(&first_out), lane_line(&second_out));
    assert_eq!(first, fx.graph_bytes());
}

const IMPORT_IDENTITY: &str = "external-file:../home/Directory.Build.props";

fn admit_with_import(fx: &Fixture) {
    let original = b"<Project><PropertyGroup /></Project>\n";
    fs::write(fx.home.join("Directory.Build.props"), original).expect("write import");
    let stdout = admit_fresh(
        fx,
        vec![json!({"identity": IMPORT_IDENTITY, "hash": sha1_hex(original)})],
    );
    assert_eq!(
        lane_line(&stdout),
        Some("lane: enriched"),
        "an artifact carrying a recorded import is admitted and applied while the import is unchanged: {stdout}"
    );
}

#[test]
fn a_changed_msbuild_import_is_reported_as_moved_imports() {
    let fx = Fixture::build("import-changed");
    admit_with_import(&fx);
    fs::write(
        fx.home.join("Directory.Build.props"),
        "<Project><PropertyGroup><LangVersion>preview</LangVersion></PropertyGroup></Project>\n",
    )
    .expect("rewrite import");
    let stdout = fx.map(&[]);

    let g = fx.graph();
    let detail = format!("import-file-changed:{IMPORT_IDENTITY}");
    assert_not_applied(
        &stdout,
        &g,
        &format!("lane: syntax (enriched lane not applied: imports-moved: {detail})"),
        json!({"state": "not-applied", "reason": "imports-moved", "detail": detail}),
    );
    assert_eq!(g["edges"], no_artifact_edges("import-changed-twin", |_| {}));
    assert_eq!(fx.audit_lane(), "syntax");
}

#[test]
fn a_missing_msbuild_import_is_reported_as_moved_imports() {
    let fx = Fixture::build("import-missing");
    admit_with_import(&fx);
    fs::remove_file(fx.home.join("Directory.Build.props")).expect("remove import");
    let stdout = fx.map(&[]);

    let g = fx.graph();
    let detail = format!("import-file-missing:{IMPORT_IDENTITY}");
    assert_not_applied(
        &stdout,
        &g,
        &format!("lane: syntax (enriched lane not applied: imports-moved: {detail})"),
        json!({"state": "not-applied", "reason": "imports-moved", "detail": detail}),
    );
    assert_eq!(g["edges"], no_artifact_edges("import-missing-twin", |_| {}));
    assert_eq!(fx.audit_lane(), "syntax");
}

const UNREADABLE_LINE: &str = "lane: syntax (enriched lane not applied: artifact-unreadable)";

fn unreadable_record() -> Value {
    json!({"state": "not-applied", "reason": "artifact-unreadable"})
}

/// Maps once with no artifact, lets `plant` put something unusable at the
/// artifact path, maps again and checks the unreadable lane.
fn assert_unreadable_after(prefix: &str, plant: impl Fn(&Fixture)) {
    let fx = Fixture::build(prefix);
    fx.map(&[]);
    let syntax_edges = fx.graph()["edges"].clone();
    plant(&fx);
    let stdout = fx.map(&[]);

    let g = fx.graph();
    assert_not_applied(&stdout, &g, UNREADABLE_LINE, unreadable_record());
    assert_eq!(g["edges"], syntax_edges);
    assert_eq!(fx.audit_lane(), "syntax");
}

#[test]
fn an_artifact_whose_header_does_not_parse_is_reported_unreadable() {
    assert_unreadable_after("unreadable-header", |fx| {
        let mut wrong = candidate(&fx.head(), vec![], vec![]);
        wrong["format"] = json!("not-a-compiler-facts-artifact");
        fs::write(fx.artifact_path(), serde_json::to_vec(&wrong).unwrap()).unwrap();
    });
}

#[test]
fn an_artifact_whose_body_does_not_parse_is_reported_unreadable() {
    assert_unreadable_after("unreadable-body", |fx| {
        let line = fx.first_load_line();
        let full = serde_json::to_vec(&candidate(
            &fx.head(),
            vec![],
            vec![confirmed_load_site(line)],
        ))
        .unwrap();
        let cut = &full[..full.len() / 2];
        assert!(serde_json::from_slice::<Value>(cut).is_err());
        fs::write(fx.artifact_path(), cut).unwrap();
    });
}

#[test]
fn a_directory_at_the_artifact_path_is_reported_unreadable() {
    assert_unreadable_after("unreadable-directory", |fx| {
        fs::create_dir_all(fx.artifact_path()).unwrap();
    });
}

const NO_SEMANTIC_LINE: &str = "lane: syntax (enriched lane not applied: no-semantic)";

fn no_semantic_record() -> Value {
    json!({"state": "not-applied", "reason": "no-semantic"})
}

fn without_lane(mut graph: Value) -> Value {
    graph["stats"]
        .as_object_mut()
        .expect("stats object")
        .remove("lane");
    graph
}

#[test]
fn no_semantic_over_an_enriched_graph_prints_the_no_semantic_lane() {
    let fx = Fixture::build("no-semantic-enriched");
    fx.map(&[]);
    let no_artifact_bytes = fx.graph_bytes();
    let line = fx.first_load_line();
    fx.import(&candidate(
        &fx.head(),
        vec![],
        vec![confirmed_load_site(line)],
    ));
    assert_eq!(lane_line(&fx.map(&[])), Some("lane: enriched"));

    let stdout = fx.map(&["--no-semantic"]);
    let g = fx.graph();
    assert_not_applied(&stdout, &g, NO_SEMANTIC_LINE, no_semantic_record());
    let no_artifact: Value = serde_json::from_slice(&no_artifact_bytes).unwrap();
    assert_eq!(without_lane(g.clone()), no_artifact);

    let lane_key = r#","lane":{"state":"not-applied","reason":"no-semantic"}"#;
    let text = String::from_utf8(fx.graph_bytes()).unwrap();
    assert_eq!(
        text.replacen(lane_key, "", 1).into_bytes(),
        no_artifact_bytes,
        "apart from its lane record the graph is byte-identical to the no-artifact graph"
    );
}

#[test]
fn no_semantic_without_an_artifact_records_the_lane_on_every_run() {
    let fx = Fixture::build("no-semantic-absent");
    for _ in 0..2 {
        let stdout = fx.map(&["--no-semantic"]);
        assert!(stdout.contains("graph rebuilt in"), "{stdout}");
        assert_not_applied(&stdout, &fx.graph(), NO_SEMANTIC_LINE, no_semantic_record());
    }
}

#[test]
fn no_semantic_never_reads_a_garbage_artifact() {
    let fx = Fixture::build("no-semantic-garbage");
    fx.map(&[]);
    fs::write(fx.artifact_path(), b"not json").unwrap();
    let stdout = fx.map(&["--no-semantic"]);
    assert_not_applied(&stdout, &fx.graph(), NO_SEMANTIC_LINE, no_semantic_record());
}

#[test]
fn no_artifact_and_no_flag_keep_the_one_line_output_and_no_lane_key() {
    let fx = Fixture::build("no-artifact");
    for _ in 0..2 {
        let stdout = fx.map(&[]);
        assert_eq!(stdout.lines().count(), 1, "{stdout}");
        assert!(stdout.starts_with("mapped "), "{stdout}");
        let stats = &fx.graph()["stats"];
        assert!(stats.get("lane").is_none(), "{stats}");
        assert!(stats.get("semantic").is_none(), "{stats}");
    }
    assert_eq!(fx.audit_lane(), "syntax");
}
