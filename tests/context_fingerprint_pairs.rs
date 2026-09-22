//! Offline shape check over the build-context fingerprint's own mutation-
//! class evidence under `fixtures/csharp-context-fingerprint/pairs/`.
//!
//! Every pair is generated once, locally, and committed; this file never
//! invokes `dotnet`, mirroring `tests/context_envelope.rs`'s own pattern.
//! It pins the one property each pair exists to prove: the record named in
//! `identity` (or `identity.projectName`, when a pair's identity itself is
//! expected to differ, such as build configuration) carries the same
//! `documents` on both sides -- the consuming source file's own inventory
//! never moved -- while `fingerprint` differs.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/csharp-context-fingerprint/pairs")
        .join(name)
}

fn load(name: &str) -> Map<String, Value> {
    let text = fs::read_to_string(fixture(name)).unwrap_or_else(|e| panic!("read {name}: {e}"));
    assert!(
        text.ends_with('\n') && !text.contains('\r'),
        "{name}: LF-terminated"
    );
    match serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: parses as JSON: {e}")) {
        Value::Object(map) => map,
        other => panic!("{name}: root is not an object: {other}"),
    }
}

fn record<'a>(doc: &'a Map<String, Value>, project_name: &str) -> &'a Map<String, Value> {
    doc["compilations"]
        .as_array()
        .expect("compilations is an array")
        .iter()
        .map(|r| r.as_object().expect("a compilation record is an object"))
        .find(|r| r["identity"]["projectName"] == project_name)
        .unwrap_or_else(|| panic!("{project_name}: record present"))
}

/// One mutation-class pair: `before`/`after` are pairs' file names (relative
/// to `pairs/`), `project` is the record to compare, and `identity_moves`
/// says whether the pair's own identity is expected to differ too (true
/// only for the build-configuration class, where `configuration` is part
/// of identity by design).
struct Pair {
    label: &'static str,
    before: &'static str,
    after: &'static str,
    project: &'static str,
    identity_moves: bool,
}

const PAIRS: &[Pair] = &[
    Pair {
        label: "metadata reference content",
        before: "class1-metadata-reference-content-before.json",
        after: "class1-metadata-reference-content-after.json",
        project: "RefProbe",
        identity_moves: false,
    },
    Pair {
        label: "build configuration",
        before: "base.json",
        after: "class2-build-configuration-after.json",
        project: "Probe",
        identity_moves: true,
    },
    Pair {
        label: "preprocessor symbol",
        before: "base.json",
        after: "class3-preprocessor-symbol-after.json",
        project: "Probe",
        identity_moves: false,
    },
    Pair {
        label: "language option",
        before: "base.json",
        after: "class4-language-option-after.json",
        project: "Probe",
        identity_moves: false,
    },
    Pair {
        label: "SDK/MSBuild version",
        before: "class5-sdk-version-before.json",
        after: "class5-sdk-version-after.json",
        project: "SdkPair",
        identity_moves: false,
    },
    Pair {
        label: "generator input",
        before: "base.json",
        after: "class6a-generator-input-after.json",
        project: "Probe",
        identity_moves: false,
    },
    Pair {
        label: "analyzer reference",
        before: "base.json",
        after: "class6b-analyzer-reference-after.json",
        project: "Probe",
        identity_moves: false,
    },
    Pair {
        label: "dependency compilation's own fingerprint",
        before: "base.json",
        after: "class7-dependency-fingerprint-after.json",
        project: "Probe",
        identity_moves: false,
    },
];

#[test]
fn every_mutation_class_pair_holds_documents_fixed_and_moves_the_fingerprint() {
    for pair in PAIRS {
        let before_doc = load(pair.before);
        let after_doc = load(pair.after);
        let before = record(&before_doc, pair.project);
        let after = record(&after_doc, pair.project);

        assert_eq!(
            before["documents"], after["documents"],
            "{}: the consuming source file's own inventory moved",
            pair.label
        );

        if pair.identity_moves {
            assert_ne!(
                before["identity"], after["identity"],
                "{}: identity was expected to move too",
                pair.label
            );
        } else {
            assert_eq!(
                before["identity"], after["identity"],
                "{}: identity moved unexpectedly",
                pair.label
            );
        }

        let before_fp = before["fingerprint"]
            .as_str()
            .expect("before fingerprint is a string");
        let after_fp = after["fingerprint"]
            .as_str()
            .expect("after fingerprint is a string");
        assert_ne!(
            before_fp, after_fp,
            "{}: fingerprint did not move",
            pair.label
        );
        assert_eq!(before_fp.len(), 40, "{}: a sha1 hex digest", pair.label);
        assert_eq!(after_fp.len(), 40, "{}: a sha1 hex digest", pair.label);
    }
}

#[test]
fn the_sdk_version_pair_genuinely_differs_in_resolved_sdk_and_msbuild() {
    let before = load("class5-sdk-version-before.json");
    let after = load("class5-sdk-version-after.json");
    let before_record = record(&before, "SdkPair");
    let after_record = record(&after, "SdkPair");

    let before_versions = before_record["versions"].as_object().unwrap();
    let after_versions = after_record["versions"].as_object().unwrap();
    assert_ne!(before_versions["sdk"], after_versions["sdk"]);
    assert_ne!(before_versions["msbuild"], after_versions["msbuild"]);
    assert_eq!(before_versions["sdk"], Value::from("9.0.305"));
    assert_eq!(after_versions["sdk"], Value::from("8.0.121"));
}

#[test]
fn the_reference_content_pair_moves_through_a_real_assembly_identity_change() {
    let before = load("class1-metadata-reference-content-before.json");
    let after = load("class1-metadata-reference-content-after.json");
    let before_record = record(&before, "RefProbe");
    let after_record = record(&after, "RefProbe");

    let ext_before = before_record["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "ExtLib")
        .expect("ExtLib reference present (before)");
    let ext_after = after_record["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "ExtLib")
        .expect("ExtLib reference present (after)");
    assert_ne!(
        ext_before["identity"], ext_after["identity"],
        "the referenced assembly's own identity (mvid) moved"
    );
}

/// The same project under two targets and two configurations must
/// round-trip as four distinct identities and four distinct fingerprints
/// in one envelope. `net8.0;net9.0` (not `net472`) so every identity's
/// `configuration`/`platform` is populated -- no evaluation gap
/// contributes an incomplete tuple. Two runs merge into one logical
/// envelope the same way the sibling fixture's own test already merges
/// committed snapshots to prove a narrower two-axis case.
#[test]
fn a_project_under_two_targets_and_two_configurations_has_four_distinct_identities_and_fingerprints(
) {
    let debug = load_ac4("context-debug.json");
    let release = load_ac4("context-release.json");

    let mut records: Vec<&Map<String, Value>> = debug["compilations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_object().unwrap())
        .chain(
            release["compilations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r.as_object().unwrap()),
        )
        .collect();
    assert_eq!(records.len(), 4, "net8.0/net9.0 x Debug/Release");

    for record in &records {
        assert_eq!(record["state"], "complete");
        let identity = record["identity"].as_object().unwrap();
        for field in ["requestedTfm", "effectiveTfm", "configuration", "platform"] {
            assert!(
                identity[field].is_string() && !identity[field].as_str().unwrap().is_empty(),
                "identity.{field} is a complete, non-null tuple member: {identity:?}"
            );
        }
    }

    let mut identities: Vec<String> = records
        .iter()
        .map(|r| serde_json::to_string(&r["identity"]).unwrap())
        .collect();
    identities.sort();
    identities.dedup();
    assert_eq!(identities.len(), 4, "four distinct identities");

    let mut fingerprints: Vec<&str> = records
        .iter_mut()
        .map(|r| r["fingerprint"].as_str().unwrap())
        .collect();
    fingerprints.sort_unstable();
    fingerprints.dedup();
    assert_eq!(fingerprints.len(), 4, "four distinct fingerprints");
}

fn load_ac4(name: &str) -> Map<String, Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/csharp-context-fingerprint/ac4")
        .join(name);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"));
    match serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: parses as JSON: {e}")) {
        Value::Object(map) => map,
        other => panic!("{name}: root is not an object: {other}"),
    }
}

#[test]
fn the_dependency_fingerprint_pair_moves_through_the_project_reference_fold_only() {
    let before = load("base.json");
    let after = load("class7-dependency-fingerprint-after.json");
    let before_record = record(&before, "Probe");
    let after_record = record(&after, "Probe");

    // Probe's own direct preprocessor symbols never changed: only Shared's did.
    assert_eq!(
        before_record["preprocessorSymbols"], after_record["preprocessorSymbols"],
        "Probe's own symbols moved; this pair is meant to isolate Shared's"
    );

    let shared_ref_before = before_record["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "project" && r["name"] == "Shared")
        .expect("Shared project reference present (before)");
    let shared_ref_after = after_record["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "project" && r["name"] == "Shared")
        .expect("Shared project reference present (after)");
    assert_ne!(
        shared_ref_before["fingerprint"], shared_ref_after["fingerprint"],
        "Shared's own fingerprint did not move"
    );
}
