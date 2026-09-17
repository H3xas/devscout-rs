//! Shape tests for the committed build-context envelope snapshots under
//! `fixtures/csharp-context/`.
//!
//! The snapshots are produced by the Roslyn sidecar (`tools/scout-semantic`)
//! running in its `--emit context` mode; CI's `semantic-audit` job
//! regenerates the default one and diffs the bytes. This file never invokes
//! `dotnet`: it reads the committed documents and pins the contract a
//! downstream admission path and a downstream reject/defer decision both
//! rely on -- the five context states, their reasons, the expected/loaded/
//! dropped document accounting, the fingerprint's presence and stability,
//! and that a reject-or-defer decision over that shape is a pure function of
//! it -- mirroring `tests/flowtrace_facts.rs`'s no-dotnet-on-PATH pattern.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-context").join(name)
}

fn text(name: &str) -> String {
    let text = fs::read_to_string(fixture(name)).unwrap_or_else(|e| panic!("read {name}: {e}"));
    assert!(
        text.ends_with('\n') && !text.contains('\r'),
        "{name}: LF-terminated"
    );
    text
}

fn load(name: &str) -> Map<String, Value> {
    match serde_json::from_str(&text(name)).unwrap_or_else(|e| panic!("{name}: parses as JSON: {e}")) {
        Value::Object(map) => map,
        other => panic!("{name}: root is not an object: {other}"),
    }
}

fn records<'a>(doc: &'a Map<String, Value>) -> Vec<&'a Map<String, Value>> {
    doc["compilations"]
        .as_array()
        .expect("compilations is an array")
        .iter()
        .map(|r| r.as_object().expect("every compilation is an object"))
        .collect()
}

const ALL_FILES: &[&str] = &[
    "context.json",
    "context-tfm-net9.0.json",
    "context-tfm-net9.0-release.json",
    "context-tfm-net48.json",
    "context-scoped.json",
];

const VALID_STATES: &[&str] = &["complete", "partial", "unsupported", "failed", "excluded"];

#[test]
fn every_committed_envelope_parses_offline_and_has_header_keys_in_order() {
    for name in ALL_FILES {
        let t = text(name);
        let doc = load(name);
        let keys: Vec<&str> = t.lines().take(6).filter_map(|l| {
            let l = l.trim_start();
            let l = l.strip_prefix('"')?;
            let end = l.find('"')?;
            Some(&l[..end])
        }).collect();
        assert_eq!(
            keys,
            ["schemaVersion", "producer", "version", "repo", "solution"],
            "{name}: header key order"
        );
        assert_eq!(doc["schemaVersion"], Value::from(1), "{name}: schemaVersion");
        assert_eq!(doc["producer"], Value::from("scout-semantic"), "{name}: producer");
        assert_eq!(doc["repo"], Value::from("csharp-context"), "{name}: repo");
        assert_eq!(doc["solution"], Value::from("Fixture.sln"), "{name}: solution");
        assert!(!records(&doc).is_empty(), "{name}: at least one compilation record");
    }
}

#[test]
fn all_five_states_are_present_across_the_committed_envelopes_each_with_a_reason() {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for name in ALL_FILES {
        let doc = load(name);
        for record in records(&doc) {
            let state = record["state"].as_str().expect("state is a string");
            assert!(
                VALID_STATES.contains(&state),
                "{name}: unknown state {state}"
            );
            seen.insert(state.to_string());
            let reason = record["reason"].as_str().expect("reason is a string");
            assert!(!reason.is_empty(), "{name}: {state} record has an empty reason");
        }
    }
    let expected: BTreeSet<String> = VALID_STATES.iter().map(|s| s.to_string()).collect();
    assert_eq!(seen, expected, "every state appears somewhere in the committed set");
}

#[test]
fn a_non_null_compilation_is_never_complete_by_itself() {
    for name in ALL_FILES {
        let doc = load(name);
        for record in records(&doc) {
            if record["state"] == "complete" {
                let compiler = record["diagnostics"]["compiler"].as_array().unwrap();
                let workspace = record["diagnostics"]["workspace"].as_array().unwrap();
                let dropped = record["documents"]["dropped"].as_array().unwrap();
                assert!(
                    compiler.is_empty() && workspace.is_empty() && dropped.is_empty(),
                    "{name}: {:?} is complete but carries a diagnostic or a dropped document",
                    record["identity"]
                );
                assert!(
                    record["fingerprint"].is_string(),
                    "{name}: a complete record still carries its fingerprint"
                );
            }
        }
    }
}

#[test]
fn a_partial_record_carries_the_compilers_own_raw_diagnostic() {
    let doc = load("context.json");
    let legacy_net472 = records(&doc)
        .into_iter()
        .find(|r| {
            r["identity"]["projectName"] == "Legacy" && r["identity"]["effectiveTfm"] == "net472"
        })
        .expect("Legacy@net472 record");
    assert_eq!(legacy_net472["state"], "partial");
    assert_eq!(legacy_net472["reason"], "binding-error");
    let compiler = legacy_net472["diagnostics"]["compiler"].as_array().unwrap();
    assert!(!compiler.is_empty(), "raw compiler diagnostics, not a count");
    let first = compiler[0].as_object().unwrap();
    assert_eq!(first["severity"], "Error");
    assert!(first["id"].as_str().unwrap().starts_with("CS"));
    assert!(!first["message"].as_str().unwrap().is_empty());
    // A default (non-strict) run still writes the envelope even though this
    // record is not complete.
    assert!(fixture("context.json").exists());
}

#[test]
fn unsupported_target_names_both_sides_and_carries_zero_facts_under_that_identity() {
    let doc = load("context-tfm-net48.json");
    let all: Vec<&Map<String, Value>> = records(&doc)
        .into_iter()
        .filter(|r| r["identity"]["projectName"] != "Vanished")
        .collect();
    assert_eq!(all.len(), 3, "one unsupported record per real project, none silently dropped");
    for record in &all {
        assert_eq!(record["state"], "unsupported");
        assert_eq!(record["reason"], "undeclared-target");
        assert_eq!(record["identity"]["requestedTfm"], "net48");
        assert!(record["identity"]["effectiveTfm"].is_null());
        let declared = record["identity"]["declaredTfms"]
            .as_array()
            .expect("declaredTfms is present on an unsupported record");
        assert!(!declared.is_empty(), "the declared side is named too");
        assert!(
            declared.iter().all(|t| t != "net48"),
            "net48 is exactly the target that was not declared"
        );
        assert!(record["fingerprint"].is_null(), "no compilation, no fingerprint");
        assert!(record["references"].as_array().unwrap().is_empty());
        assert!(record["documents"]["loaded"].as_array().unwrap().is_empty());
        assert!(
            record["generated"]["documents"].as_array().unwrap().is_empty(),
            "zero facts under an unsupported identity"
        );
    }
    let legacy = all
        .iter()
        .find(|r| r["identity"]["projectName"] == "Legacy")
        .unwrap();
    let declared: Vec<&str> = legacy["identity"]["declaredTfms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(declared, ["net472", "net9.0"], "both of Legacy's real variants are named");
}

#[test]
fn every_document_drop_reason_is_exercised_and_expected_is_a_superset_of_loaded_and_dropped() {
    let mut reasons: BTreeSet<String> = BTreeSet::new();
    for name in ALL_FILES {
        let doc = load(name);
        for record in records(&doc) {
            let documents = &record["documents"];
            let expected: BTreeSet<&str> = documents["expected"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            let loaded: BTreeSet<&str> = documents["loaded"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            let dropped = documents["dropped"].as_array().unwrap();
            for entry in dropped {
                let entry = entry.as_object().unwrap();
                let path = entry["path"].as_str().unwrap();
                let reason = entry["reason"].as_str().unwrap();
                assert!(!reason.is_empty(), "{name}: a dropped document names its reason");
                reasons.insert(reason.to_string());
                assert!(
                    expected.contains(path),
                    "{name}: dropped path {path} is still named in expected"
                );
            }
            // Roslyn's own document walk tolerates a `Compile` item whose
            // file does not exist on disk at all: it hands back an empty
            // document rather than skipping it, so a "missing" document can
            // be both "loaded" (as an empty tree) and "dropped" (with the
            // reason "missing") at once (see the dedicated test below). The
            // other three reasons make `RepoPaths.Classify` return no
            // relative path at all, so Roslyn's own walk -- which uses that
            // same classification -- never adds those paths to `loaded`.
            let other_dropped_paths: BTreeSet<&str> = dropped
                .iter()
                .filter(|d| d["reason"] != "missing")
                .map(|d| d["path"].as_str().unwrap())
                .collect();
            for loaded_path in &loaded {
                assert!(
                    expected.contains(loaded_path),
                    "{name}: loaded path {loaded_path} is named in expected too"
                );
                assert!(
                    !other_dropped_paths.contains(loaded_path),
                    "{name}: {loaded_path} is dropped for a reason other than missing, so it cannot also be loaded"
                );
            }
        }
    }
    let expected_reasons: BTreeSet<String> =
        ["missing", "linked-outside-root", "skipped-directory", "out-of-scope"]
            .iter()
            .map(|s| s.to_string())
            .collect();
    assert_eq!(reasons, expected_reasons, "all four drop reasons appear somewhere");
}

#[test]
fn a_document_roslyn_silently_treats_as_loaded_is_still_reported_missing() {
    let doc = load("context.json");
    let clean = records(&doc)
        .into_iter()
        .find(|r| r["identity"]["projectName"] == "Clean")
        .expect("Clean record");
    let loaded: Vec<&str> = clean["documents"]["loaded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        loaded.contains(&"src/Clean/Ghost.cs"),
        "Roslyn's own document walk hands back an empty document for a Compile \
         item whose file was never created, rather than skipping it"
    );
    let missing: Vec<&str> = clean["documents"]["dropped"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["reason"] == "missing")
        .map(|d| d["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        missing, ["src/Clean/Ghost.cs"],
        "the independent, workspace-free inventory still names it missing"
    );
    // The whole point: a document Roslyn silently "loaded" as empty content
    // does not let the record stay complete.
    assert_ne!(clean["state"], "complete");
}

#[test]
fn a_project_a_solution_names_but_that_never_reaches_the_workspace_is_still_reported() {
    for name in ALL_FILES {
        let doc = load(name);
        let vanished = records(&doc)
            .into_iter()
            .find(|r| r["identity"]["projectName"] == "Vanished")
            .unwrap_or_else(|| panic!("{name}: the Vanished project is reported"));
        assert_eq!(vanished["state"], "failed", "{name}");
        assert_eq!(vanished["reason"], "project-not-loaded", "{name}");
        assert!(vanished["identity"]["requestedTfm"].is_null());
        assert!(vanished["identity"]["effectiveTfm"].is_null());
        assert!(vanished["fingerprint"].is_null(), "{name}: no compilation, no fingerprint");
        assert_eq!(
            vanished["identity"]["projectPath"], "src/Vanished/Vanished.csproj",
            "{name}: named from the solution file itself, not from a workspace that never saw it"
        );
    }
}

#[test]
fn generated_documents_are_accounted_separately_and_a_partial_sibling_does_not_poison_a_complete_one() {
    let doc = load("context-tfm-net9.0.json");
    let all = records(&doc);
    let clean = all
        .iter()
        .find(|r| r["identity"]["projectName"] == "Clean")
        .expect("Clean record");
    let generated = clean["generated"]["documents"].as_array().unwrap();
    assert!(!generated.is_empty(), "Clean's source-generated document is inventoried");
    for entry in generated {
        let entry = entry.as_object().unwrap();
        assert!(!entry["hintName"].as_str().unwrap().is_empty());
        // The Workspace API exposes no generator-identity property (verified
        // against the restored 4.14.0 assemblies, recorded in the
        // implementation journal): every entry is "unknown", not a guess.
        assert_eq!(entry["generator"], "unknown");
    }

    let legacy = all
        .iter()
        .find(|r| r["identity"]["projectName"] == "Legacy")
        .expect("Legacy record");
    assert_eq!(legacy["state"], "complete", "Legacy binds cleanly under net9.0");

    let broken = all
        .iter()
        .find(|r| r["identity"]["projectName"] == "Broken")
        .expect("Broken record");
    assert_ne!(broken["state"], "complete", "Broken's own reference is still unresolved");

    // Legacy stays fully inspectable (a real fingerprint, a real versions
    // block) even though Broken, in the very same envelope, is not complete.
    assert!(legacy["fingerprint"].is_string());
    assert!(legacy["versions"].is_object());
}

#[test]
fn two_targets_of_one_project_are_distinct_identities_with_distinct_fingerprints() {
    let default_doc = load("context.json");
    let net9_doc = load("context-tfm-net9.0.json");

    let legacy_net472 = records(&default_doc)
        .into_iter()
        .find(|r| r["identity"]["projectName"] == "Legacy" && r["identity"]["effectiveTfm"] == "net472")
        .expect("Legacy@net472");
    let legacy_net9 = records(&net9_doc)
        .into_iter()
        .find(|r| r["identity"]["projectName"] == "Legacy" && r["identity"]["effectiveTfm"] == "net9.0")
        .expect("Legacy@net9.0");

    assert_ne!(legacy_net472["identity"], legacy_net9["identity"], "distinct identities");
    let fp_net472 = legacy_net472["fingerprint"].as_str().unwrap();
    let fp_net9 = legacy_net9["fingerprint"].as_str().unwrap();
    assert_ne!(fp_net472, fp_net9, "distinct fingerprints");
    assert_eq!(fp_net472.len(), 40, "a sha1 hex digest");
    assert_eq!(fp_net9.len(), 40, "a sha1 hex digest");
}

#[test]
fn two_configurations_of_one_target_are_distinct_identities_with_distinct_fingerprints() {
    let debug_doc = load("context-tfm-net9.0.json");
    let release_doc = load("context-tfm-net9.0-release.json");

    let legacy_debug = records(&debug_doc)
        .into_iter()
        .find(|r| r["identity"]["projectName"] == "Legacy")
        .expect("Legacy (Debug)");
    let legacy_release = records(&release_doc)
        .into_iter()
        .find(|r| r["identity"]["projectName"] == "Legacy")
        .expect("Legacy (Release)");

    assert_eq!(legacy_debug["identity"]["effectiveTfm"], legacy_release["identity"]["effectiveTfm"]);
    assert_ne!(
        legacy_debug["identity"]["configuration"],
        legacy_release["identity"]["configuration"],
        "the one field this pair is engineered to differ on"
    );
    assert_ne!(
        legacy_debug["identity"], legacy_release["identity"],
        "distinct identities overall"
    );
    assert_ne!(
        legacy_debug["fingerprint"].as_str().unwrap(),
        legacy_release["fingerprint"].as_str().unwrap(),
        "distinct fingerprints"
    );
}

/// A reject/defer decision over an admitted, partial, stale or unsupported
/// envelope record is a pure function of the record: no engine runner,
/// artifact admission, cache or resolver merge exists here or is implied by
/// this test -- it exists only to prove the envelope's shape is sufficient
/// for a consumer to decide without resolver knowledge, per this ticket's
/// own boundary against its downstream consumers.
fn decide(record: &Map<String, Value>, prior_fingerprint: Option<&str>) -> &'static str {
    let state = record["state"].as_str().unwrap();
    match state {
        "failed" | "unsupported" => "reject",
        "partial" => "defer",
        "excluded" => "defer",
        "complete" => {
            let fingerprint = record["fingerprint"].as_str().unwrap();
            match prior_fingerprint {
                Some(prior) if prior != fingerprint => "defer",
                _ => "admit",
            }
        }
        other => panic!("unknown state {other}"),
    }
}

#[test]
fn reject_or_defer_decision_is_a_pure_function_of_the_envelope() {
    let doc = load("context-tfm-net9.0.json");
    let all = records(&doc);

    let legacy = all.iter().find(|r| r["identity"]["projectName"] == "Legacy").unwrap();
    assert_eq!(decide(legacy, None), "admit", "a fresh complete record admits");
    let fp = legacy["fingerprint"].as_str().unwrap().to_string();
    assert_eq!(decide(legacy, Some(&fp)), "admit", "an unchanged fingerprint admits");
    assert_eq!(decide(legacy, Some("stale-fingerprint")), "defer", "a moved fingerprint is stale");

    let broken = all.iter().find(|r| r["identity"]["projectName"] == "Broken").unwrap();
    assert_eq!(decide(broken, None), "defer", "partial defers");

    let unsupported_doc = load("context-tfm-net48.json");
    let unsupported = records(&unsupported_doc)
        .into_iter()
        .find(|r| r["state"] == "unsupported")
        .expect("an unsupported record exists");
    assert_eq!(decide(unsupported, None), "reject", "unsupported rejects");

    let vanished = records(&unsupported_doc)
        .into_iter()
        .find(|r| r["state"] == "failed")
        .expect("a failed record exists");
    assert_eq!(decide(vanished, None), "reject", "failed rejects");

    let default_doc = load("context.json");
    let excluded = records(&default_doc)
        .into_iter()
        .find(|r| r["state"] == "excluded")
        .expect("an excluded record exists");
    assert_eq!(decide(excluded, None), "defer");

    // Calling decide() twice on the same inputs gives the same answer: it
    // reads only the record (and the caller-supplied prior fingerprint),
    // nothing external, nothing mutable.
    assert_eq!(decide(legacy, None), decide(legacy, None));
}

#[test]
fn versions_and_engine_are_present_on_every_non_unsupported_record() {
    for name in ALL_FILES {
        let doc = load(name);
        for record in records(&doc) {
            if record["state"] == "unsupported" || record["state"] == "excluded" {
                continue;
            }
            let versions = record["versions"]
                .as_object()
                .unwrap_or_else(|| panic!("{name}: {:?} carries versions", record["identity"]));
            for key in ["sdk", "msbuild", "compiler", "engine"] {
                let value = versions[key].as_str().unwrap_or_else(|| panic!("{name}: versions.{key}"));
                assert!(!value.is_empty(), "{name}: versions.{key} is non-empty");
            }
        }
    }
}
