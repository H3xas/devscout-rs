//! The refusal half of `fact_export`'s acceptance: the one refusal path, the
//! reason words it prints, and the labels an index that is merely out of date
//! keeps instead.

#[path = "fact_export_support/mod.rs"]
mod support;

use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use support::{family_lines, header_of, read_json, Fixture};

fn assert_refused(fixture: &Fixture, expected: &str, target: Option<&PathBuf>) {
    let mut args = vec!["export"];
    let target_text = target.map(|path| path.to_string_lossy().to_string());
    if let Some(text) = target_text.as_deref() {
        args.push("--out");
        args.push(text);
    }
    let out = fixture.run(&args);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{expected}: a refusal is an environment failure, not a usage error"
    );
    assert!(
        out.stdout.is_empty(),
        "{expected}: a refusal writes no stream"
    );
    let stderr = String::from_utf8(out.stderr).expect("stderr is UTF-8");
    let stderr = stderr.trim_end();
    assert_eq!(
        stderr.lines().count(),
        1,
        "{expected}: exactly one stderr line, got {stderr}"
    );
    assert!(
        stderr.contains(&format!("({expected})")),
        "the reason word is stated: {stderr}"
    );
    assert!(
        stderr.contains("devscout map"),
        "every refusal names the one recovery: {stderr}"
    );
    if let Some(path) = target {
        assert!(!path.exists(), "{expected}: no --out file appears");
    }
}

#[test]
fn fact_export_ac_6_an_unmapped_repository_refuses_with_no_index() {
    let fixture = Fixture::unmapped("csharp");
    assert_refused(&fixture, "no-index", None);
}

#[test]
fn fact_export_ac_6_a_directory_outside_any_repository_refuses_with_no_index() {
    let dir =
        std::env::temp_dir().join(format!("scout-fact-export-outside-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the outside directory");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_devscout"))
        .args(["export"])
        .current_dir(&dir)
        .output()
        .expect("devscout must run");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("(no-index)"), "{stderr}");
    assert!(stderr.contains("devscout map"), "{stderr}");
    assert!(out.stdout.is_empty());
}

#[test]
fn fact_export_ac_6_a_graph_with_no_published_record_refuses_with_no_index_state() {
    let fixture = Fixture::build("csharp");
    fs::remove_file(fixture.state_path().join("index-state.json")).expect("remove the record");
    assert_refused(&fixture, "no-index-state", None);
}

#[test]
fn fact_export_ac_6_an_uncertifiable_record_refuses_with_legacy_index() {
    let fixture = Fixture::build("csharp");
    fs::write(
        fixture.state_path().join("index-state.json"),
        r#"{"head":"abc","dirty":false,"indexed_at":"x","file_count":1,"dirty_indexed_files":[]}"#,
    )
    .expect("write the legacy record");
    assert_refused(&fixture, "legacy-index", None);
}

#[test]
fn fact_export_ac_6_a_missing_digest_table_refuses_with_legacy_index() {
    let fixture = Fixture::build("csharp");
    fs::remove_file(fixture.state_path().join("index-digests.json")).expect("remove the digests");
    assert_refused(&fixture, "legacy-index", None);
}

#[test]
fn fact_export_ac_6_a_removed_graph_refuses_with_artifact_missing() {
    let fixture = Fixture::build("csharp");
    let target = fixture.root.join("stream.ndjson");
    fs::remove_file(fixture.state_path().join("graph/graph.json")).expect("remove the graph");
    assert_refused(&fixture, "artifact-missing", Some(&target));
}

#[test]
fn fact_export_ac_6_a_truncated_graph_refuses_with_artifact_unreadable() {
    let fixture = Fixture::build("csharp");
    let graph = fixture.state_path().join("graph/graph.json");
    let bytes = fs::read(&graph).expect("read the graph");
    fs::write(&graph, &bytes[..bytes.len() / 2]).expect("truncate the graph");
    assert_refused(&fixture, "artifact-unreadable", None);
}

#[test]
fn fact_export_ac_6_a_truncated_fragment_cache_refuses_with_artifact_unreadable() {
    let fixture = Fixture::build("csharp");
    let cache = fixture.cache_path();
    let bytes = fs::read(&cache).expect("read the cache");
    fs::write(&cache, &bytes[..bytes.len() / 2]).expect("truncate the cache");
    assert_refused(&fixture, "artifact-unreadable", None);
}

#[test]
fn fact_export_ac_6_an_older_graph_schema_refuses_with_unknown_generation() {
    let fixture = Fixture::build("csharp");
    let graph = fixture.state_path().join("graph/graph.json");
    let mut value = read_json(&graph);
    value["schema_version"] = Value::from(2);
    fs::write(
        &graph,
        serde_json::to_vec(&value).expect("serialize the graph"),
    )
    .expect("write the older graph");
    assert_refused(&fixture, "unknown-generation", None);
}

#[test]
fn fact_export_ac_6_a_cache_of_another_generation_refuses_with_unknown_generation() {
    let fixture = Fixture::build("csharp");
    let cache = fixture.cache_path();
    let older = cache.with_file_name("fragments-v37.json");
    fs::rename(&cache, &older).expect("rename the cache to an older generation");
    assert_refused(&fixture, "unknown-generation", None);
}

#[test]
fn fact_export_ac_6_a_map_in_progress_refuses_with_index_changed() {
    let fixture = Fixture::build("csharp");
    fs::write(
        fixture.state_path().join("index-state.json"),
        r#"{"format":"devscout-index-state","version":2,"state":"in-progress","owner":"/tmp/git","started_at":"2026-01-01T00:00:00Z"}"#,
    )
    .expect("write the marker");
    assert_refused(&fixture, "index-changed", None);
}

#[test]
fn fact_export_ac_6_a_digest_table_from_another_run_refuses_with_index_changed() {
    let fixture = Fixture::build("csharp");
    let digests = fixture.state_path().join("index-digests.json");
    let mut value = read_json(&digests);
    value["generation"] = Value::from("another-generation");
    fs::write(
        &digests,
        serde_json::to_vec(&value).expect("serialize the digests"),
    )
    .expect("write the other generation");
    assert_refused(&fixture, "index-changed", None);
}

#[test]
fn fact_export_ac_6_a_stale_index_exports_with_its_label() {
    let fixture = Fixture::build("rust");
    let source = fixture.root.join("src/shelf.rs");
    let edited = fs::read_to_string(&source).expect("read the source");
    fs::write(&source, format!("{edited}\n// changed\n")).expect("edit the source");
    let lines = fixture.lines();
    let freshness = header_of(&lines)["freshness"].clone();
    assert_eq!(freshness["state"], "stale", "{freshness}");
    assert!(
        freshness["currentHead"].is_string() && freshness["changedFiles"].is_number(),
        "the stale label carries the vocabulary a query answers with: {freshness}"
    );
}

#[test]
fn fact_export_ac_6_another_worktree_s_index_exports_with_its_label_and_keeps_the_messaging_facts()
{
    let fixture = Fixture::build("csharp");
    let linked = fixture.root.join("linked");
    fixture.git(&[
        "worktree",
        "add",
        "-q",
        linked.to_str().expect("utf-8 path"),
        "--detach",
    ]);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_devscout"))
        .args(["export"])
        .current_dir(&linked)
        .env("HOME", &fixture.home)
        .env("SCOUT_REGISTRY", fixture.home.join("repos.json"))
        .env("SCOUT_CONTENT_DB", fixture.home.join("content.db"))
        .output()
        .expect("devscout must run");
    assert!(
        out.status.success(),
        "another worktree's index still exports"
    );
    let text = String::from_utf8(out.stdout).expect("UTF-8");
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    let freshness = header_of(&lines)["freshness"].clone();
    assert_eq!(freshness["state"], "unknown", "{freshness}");
    assert_eq!(freshness["reason"], "other-worktree", "{freshness}");
    assert!(
        !family_lines(&lines, "publish-site").is_empty(),
        "the messaging facts are keyed to the record's owner, not the reading worktree"
    );
}
