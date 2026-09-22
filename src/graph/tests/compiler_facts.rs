use super::*;
use serde_json::{json, Value};
use std::fs;

fn valid_expectations() -> AdmissionExpectations {
    AdmissionExpectations {
        contract_version: COMPILER_FACTS_CONTRACT_VERSION,
        producer_name: EXPECTED_PRODUCER_NAME,
        engine_revision: EXPECTED_ENGINE_REVISION,
        dependency_fingerprint: EXPECTED_DEPENDENCY_FINGERPRINT,
        context_schema_version: EXPECTED_CONTEXT_SCHEMA_VERSION,
        profile: Profile {
            target: "net9.0".to_string(),
            configuration: "Debug".to_string(),
            platform: "AnyCPU".to_string(),
        },
        head_sha: Some("a".repeat(40)),
    }
}

/// One compilation, the shape the real build-context envelope carries:
/// `identity` an opaque object, `fingerprint` a hex string (or, for
/// `unsupported`, `None`/JSON `null`).
fn valid_compilations() -> Vec<CompilationRef> {
    vec![CompilationRef {
        identity: json!({"projectPath": "Api.csproj", "projectName": "Api", "requestedTfm": "net9.0"}),
        fingerprint: Some("cafe".to_string()),
    }]
}

/// The `context.envelope` value the real producer writes: a top-level
/// `compilations` list, deliberately with **no** top-level `fingerprint`
/// field -- the exact shape the pre-delta self-referential check could
/// never have admitted, and this admission path's own real target.
fn envelope_value(compilations: &[CompilationRef]) -> Value {
    json!({
        "compilations": compilations
            .iter()
            .map(|c| json!({"identity": c.identity, "fingerprint": c.fingerprint}))
            .collect::<Vec<_>>(),
    })
}

fn candidate_with_compilations(compilations: &[CompilationRef]) -> Value {
    json!({
        "format": COMPILER_FACTS_FORMAT,
        "contractVersion": COMPILER_FACTS_CONTRACT_VERSION,
        "artifactSchemaVersion": COMPILER_FACTS_ARTIFACT_SCHEMA_VERSION,
        "producer": {"name": "scout-semantic", "engineRevision": EXPECTED_ENGINE_REVISION},
        "profile": {"target": "net9.0", "configuration": "Debug", "platform": "AnyCPU"},
        "dependencyFingerprint": EXPECTED_DEPENDENCY_FINGERPRINT,
        "context": {
            "schemaVersion": EXPECTED_CONTEXT_SCHEMA_VERSION,
            "contextFingerprint": recompute_context_summary(compilations),
            "envelope": envelope_value(compilations)
        },
        "sourceSnapshot": {"headSha": "a".repeat(40)},
        "capabilities": {"requested": ["symbols"], "provided": ["symbols"]},
        "completion": {"terminal": true},
        "units": {"processed": ["Api|net9.0"], "missing": []},
        "coverage": {"state": "complete"},
        "diagnostics": [],
        "symbols": []
    })
}

fn valid_candidate() -> Value {
    candidate_with_compilations(&valid_compilations())
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

#[test]
fn a_well_formed_candidate_is_admitted() {
    let facts = admit(&bytes(&valid_candidate()), &valid_expectations()).unwrap();
    assert!(facts.coverage().is_complete());
    assert_eq!(facts.header.engine_revision, EXPECTED_ENGINE_REVISION);
}

#[test]
fn malformed_json_is_refused() {
    let err = admit(b"not json", &valid_expectations()).unwrap_err();
    assert_eq!(err, RefusalReason::MalformedEncoding);
}

#[test]
fn a_missing_completion_record_is_refused_before_any_identity_check() {
    let mut candidate = valid_candidate();
    candidate["completion"]["terminal"] = json!(false);
    // Also wrong on every identity field, to prove completion is checked first.
    candidate["producer"]["engineRevision"] = json!("bogus");
    let err = admit(&bytes(&candidate), &valid_expectations()).unwrap_err();
    assert_eq!(err, RefusalReason::MissingCompletionRecord);
}

#[test]
fn a_unit_claimed_both_processed_and_missing_is_incoherent() {
    let mut candidate = valid_candidate();
    candidate["units"]["missing"] = json!(["Api|net9.0"]);
    let err = admit(&bytes(&candidate), &valid_expectations()).unwrap_err();
    assert_eq!(err, RefusalReason::IncoherentInventory);
}

#[test]
fn every_identity_mismatch_class_is_refused_with_its_own_token() {
    let cases: &[(&str, fn(&mut Value), RefusalReason)] = &[
        (
            "contract version",
            |c| c["contractVersion"] = json!(COMPILER_FACTS_CONTRACT_VERSION + 1),
            RefusalReason::ContractVersionMismatch,
        ),
        (
            "producer",
            |c| c["producer"]["name"] = json!("some-other-engine"),
            RefusalReason::ProducerMismatch,
        ),
        (
            "engine revision",
            |c| c["producer"]["engineRevision"] = json!("bogus"),
            RefusalReason::EngineRevisionMismatch,
        ),
        (
            "profile",
            |c| c["profile"]["target"] = json!("net472"),
            RefusalReason::ProfileMismatch,
        ),
        (
            "dependency fingerprint",
            |c| c["dependencyFingerprint"] = json!("bogus"),
            RefusalReason::DependencyFingerprintMismatch,
        ),
        (
            "context envelope version",
            |c| c["context"]["schemaVersion"] = json!(EXPECTED_CONTEXT_SCHEMA_VERSION + 1),
            RefusalReason::ContextEnvelopeVersionUnrecognised,
        ),
        (
            "context fingerprint",
            |c| c["context"]["envelope"]["compilations"][0]["fingerprint"] = json!("different"),
            RefusalReason::ContextFingerprintMismatch,
        ),
        (
            "source snapshot",
            |c| c["sourceSnapshot"]["headSha"] = json!("b".repeat(40)),
            RefusalReason::SourceSnapshotMismatch,
        ),
    ];
    for (label, mutate, expected_reason) in cases {
        let mut candidate = valid_candidate();
        mutate(&mut candidate);
        let err = admit(&bytes(&candidate), &valid_expectations()).unwrap_err();
        assert_eq!(err, *expected_reason, "case: {label}");
    }
}

#[test]
fn a_contract_version_mismatch_is_reported_before_an_engine_revision_mismatch() {
    // First-failing-check-wins: wrong on both, but contract version is
    // checked first.
    let mut candidate = valid_candidate();
    candidate["contractVersion"] = json!(COMPILER_FACTS_CONTRACT_VERSION + 1);
    candidate["producer"]["engineRevision"] = json!("bogus");
    let err = admit(&bytes(&candidate), &valid_expectations()).unwrap_err();
    assert_eq!(err, RefusalReason::ContractVersionMismatch);
}

#[test]
fn a_declared_incomplete_coverage_is_admitted_not_refused() {
    let mut candidate = valid_candidate();
    candidate["coverage"] = json!({
        "state": "incomplete",
        "incompleteUnits": [{"unit": "Api|net9.0", "reason": "one binding failed"}]
    });
    let facts = admit(&bytes(&candidate), &valid_expectations()).unwrap();
    match facts.coverage() {
        Coverage::Incomplete { units } => {
            assert_eq!(units.len(), 1);
            assert_eq!(units[0].unit, "Api|net9.0");
        }
        Coverage::Complete => panic!("expected incomplete coverage"),
    }
}

#[test]
fn no_source_snapshot_expectation_skips_the_source_snapshot_check() {
    let mut expected = valid_expectations();
    expected.head_sha = None;
    let mut candidate = valid_candidate();
    candidate["sourceSnapshot"]["headSha"] = json!("mismatched");
    admit(&bytes(&candidate), &expected).expect("no expected head_sha admits regardless");
}

#[test]
fn a_context_fingerprint_missing_on_either_side_is_refused_not_silently_skipped() {
    // The class must stay live even once a producer stops emitting one side
    // of the pairing (or both): silently admitting a candidate with nothing
    // to compare would let this identity check go inert without any test
    // failing to say so.
    let cases: &[(&str, fn(&mut Value))] = &[
        ("header side only", |c| {
            c["context"]
                .as_object_mut()
                .unwrap()
                .remove("contextFingerprint");
        }),
        ("envelope side only", |c| {
            c["context"]["envelope"]
                .as_object_mut()
                .unwrap()
                .remove("compilations");
        }),
        ("neither side", |c| {
            c["context"]
                .as_object_mut()
                .unwrap()
                .remove("contextFingerprint");
            c["context"]["envelope"]
                .as_object_mut()
                .unwrap()
                .remove("compilations");
        }),
    ];
    for (label, mutate) in cases {
        let mut candidate = valid_candidate();
        mutate(&mut candidate);
        let err = admit(&bytes(&candidate), &valid_expectations()).unwrap_err();
        assert_eq!(
            err,
            RefusalReason::ContextFingerprintMismatch,
            "case: {label}"
        );
    }
}

#[test]
fn truncation_at_any_byte_offset_is_refused_and_never_admitted() {
    // Truncation at an arbitrary byte offset is its own broken-run case,
    // distinct from a clean kill, a timeout, or a size cap: this sweeps
    // several offsets through a well-formed candidate's own bytes to prove
    // none of them is ever admitted, whichever refusal token a given cut
    // happens to trip.
    let full = bytes(&valid_candidate());
    let expected = valid_expectations();
    for offset in [
        1,
        full.len() / 5,
        full.len() / 2,
        full.len() * 3 / 4,
        full.len() - 1,
    ] {
        let truncated = &full[..offset];
        let result = admit(truncated, &expected);
        assert!(
            result.is_err(),
            "truncated candidate at offset {offset}/{} must never be admitted",
            full.len()
        );
    }
}

#[test]
fn a_truncated_republish_leaves_the_previously_admitted_artifact_byte_identical() {
    let dir = temp_dir("compiler-facts-truncate-leaves-artifact");
    let expected = valid_expectations();
    let good = bytes(&valid_candidate());
    admit_and_publish(&dir, &good, &expected).unwrap();
    let before = fs::read(compiler_facts_json_path(&dir)).unwrap();

    let truncated = &good[..good.len() * 2 / 3];
    let err = admit_and_publish(&dir, truncated, &expected).unwrap_err();
    assert!(matches!(err, PublishError::Refused(_)));

    let after = fs::read(compiler_facts_json_path(&dir)).unwrap();
    assert_eq!(
        before, after,
        "a truncated candidate must never replace the previously admitted artifact"
    );
}

#[test]
fn run_and_import_reach_the_same_outcome_through_the_same_admit_call() {
    // Both `compiler-facts run` (local acquisition) and `compiler-facts
    // import` (a build/CI-produced artifact) hand their candidate bytes to
    // the identical `admit` function -- there is no second code path.
    let candidate = bytes(&valid_candidate());
    let expected = valid_expectations();
    let from_run = admit(&candidate, &expected).unwrap();
    let from_import = admit(&candidate, &expected).unwrap();
    assert_eq!(from_run.bytes, from_import.bytes);
    assert_eq!(
        from_run.header.engine_revision,
        from_import.header.engine_revision
    );
    assert_eq!(from_run.coverage(), from_import.coverage());
}

#[test]
fn admit_and_publish_writes_the_original_candidate_bytes_verbatim() {
    let dir = temp_dir("compiler-facts-publish");
    let candidate = bytes(&valid_candidate());
    let expected = valid_expectations();
    admit_and_publish(&dir, &candidate, &expected).unwrap();
    let written = fs::read(compiler_facts_json_path(&dir)).unwrap();
    assert_eq!(
        written, candidate,
        "published bytes must be the original candidate, unchanged"
    );
}

#[test]
fn a_refused_republish_leaves_the_previously_admitted_artifact_byte_identical() {
    let dir = temp_dir("compiler-facts-refuse-leaves-artifact");
    let expected = valid_expectations();
    let good = bytes(&valid_candidate());
    admit_and_publish(&dir, &good, &expected).unwrap();
    let before = fs::read(compiler_facts_json_path(&dir)).unwrap();

    let mut bad_candidate = valid_candidate();
    bad_candidate["producer"]["engineRevision"] = json!("bogus");
    let bad = bytes(&bad_candidate);
    let err = admit_and_publish(&dir, &bad, &expected).unwrap_err();
    assert!(matches!(
        err,
        PublishError::Refused(RefusalReason::EngineRevisionMismatch)
    ));

    let after = fs::read(compiler_facts_json_path(&dir)).unwrap();
    assert_eq!(
        before, after,
        "a refusal must never touch the previously admitted artifact"
    );

    let leftover: Vec<_> = fs::read_dir(compiler_facts_json_path(&dir).parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
        .collect();
    assert!(
        leftover.is_empty(),
        "no tmp file should remain: {leftover:?}"
    );
}

#[test]
fn read_compiler_facts_is_none_when_no_artifact_was_ever_admitted() {
    let dir = temp_dir("compiler-facts-none");
    assert!(read_compiler_facts(&dir).is_none());
}

// --- per-reference occurrence facts ----------------------------------

/// One occurrence site naming `identity`/`fingerprint` as its own bound
/// compilation, with a fixed, otherwise-arbitrary shape for every other
/// field -- admission never models those, so their exact values are
/// unconstrained by anything this module checks.
fn occurrence_site(identity: &Value, fingerprint: Option<&str>) -> Value {
    occurrence_site_with_signature(identity, fingerprint, "()->void")
}

fn occurrence_site_with_signature(
    identity: &Value,
    fingerprint: Option<&str>,
    signature: &str,
) -> Value {
    json!({
        "file": "Callers.cs",
        "shape": "invocation",
        "span": {"startLine": 5, "startChar": 8, "endLine": 5, "endChar": 20},
        "name": {"line": 5, "char": 8},
        "caller": {
            "assembly": "Api", "type": "Api.Callers", "member": "Go",
            "genericArity": 0, "overloadSignature": "()->void"
        },
        "resolution": "confirmed",
        "candidateReason": "None",
        "target": {
            "assembly": "Api", "type": "Api.Widgets.Widget", "member": "Render",
            "genericArity": 0, "overloadSignature": signature
        },
        "candidates": [],
        "compilation": {"identity": identity, "fingerprint": fingerprint},
        "documentContentIdentity": "sha1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "targetDocumentContentIdentities": ["sha1:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"]
    })
}

/// A well-formed candidate, `occurrences`-capable, carrying `sites` as its
/// own `occurrences.sites` array.
fn candidate_with_occurrences(compilations: &[CompilationRef], sites: Vec<Value>) -> Value {
    let mut candidate = candidate_with_compilations(compilations);
    candidate["capabilities"]["requested"] = json!(["symbols", "diagnostics", "occurrences"]);
    candidate["capabilities"]["provided"] = json!(["symbols", "diagnostics", "occurrences"]);
    candidate["occurrences"] = json!({
        "spanEncoding": "utf16-code-unit-line1-char0-end-exclusive",
        "sites": sites,
    });
    candidate
}

#[test]
fn occurrence_capability_and_payload_disagreement_is_refused_in_both_directions() {
    let compilations = valid_compilations();

    // Capability claims occurrences are provided; the payload carries no
    // `occurrences` key at all.
    let mut claims_without_payload = candidate_with_compilations(&compilations);
    claims_without_payload["capabilities"]["provided"] =
        json!(["symbols", "diagnostics", "occurrences"]);
    let err = admit(&bytes(&claims_without_payload), &valid_expectations()).unwrap_err();
    assert_eq!(
        err,
        RefusalReason::OccurrenceCapabilityMismatch,
        "claimed but absent"
    );

    // The payload carries occurrences; the capability set does not claim it.
    let site = occurrence_site(
        &compilations[0].identity,
        compilations[0].fingerprint.as_deref(),
    );
    let mut payload_without_claim = candidate_with_occurrences(&compilations, vec![site]);
    payload_without_claim["capabilities"]["provided"] = json!(["symbols", "diagnostics"]);
    let err = admit(&bytes(&payload_without_claim), &valid_expectations()).unwrap_err();
    assert_eq!(
        err,
        RefusalReason::OccurrenceCapabilityMismatch,
        "present but unclaimed"
    );
}

#[test]
fn an_occurrence_naming_a_compilation_absent_from_the_envelope_is_refused() {
    let compilations = valid_compilations();
    let unknown_identity =
        json!({"projectPath": "Other.csproj", "projectName": "Other", "requestedTfm": "net9.0"});
    let site = occurrence_site(&unknown_identity, Some(&"d".repeat(40)));
    let candidate = candidate_with_occurrences(&compilations, vec![site]);
    let err = admit(&bytes(&candidate), &valid_expectations()).unwrap_err();
    assert_eq!(err, RefusalReason::OccurrenceCompilationUnknown);
}

#[test]
fn an_occurrence_naming_an_unsupported_compilation_is_refused() {
    let mut compilations = valid_compilations();
    let unsupported_identity =
        json!({"projectPath": "Legacy.csproj", "projectName": "Legacy", "requestedTfm": "net472"});
    compilations.push(CompilationRef {
        identity: unsupported_identity.clone(),
        fingerprint: None,
    });
    let site = occurrence_site(&unsupported_identity, None);
    let candidate = candidate_with_occurrences(&compilations, vec![site]);
    let err = admit(&bytes(&candidate), &valid_expectations()).unwrap_err();
    assert_eq!(err, RefusalReason::OccurrenceCompilationUnsupported);
}

#[test]
fn every_new_occurrence_refusal_class_leaves_the_previously_admitted_artifact_byte_identical() {
    let dir = temp_dir("compiler-facts-occurrence-refusal-leaves-artifact");
    let expected = valid_expectations();
    let good = bytes(&valid_candidate());
    admit_and_publish(&dir, &good, &expected).unwrap();
    let before = fs::read(compiler_facts_json_path(&dir)).unwrap();

    let compilations = valid_compilations();
    let unknown_identity =
        json!({"projectPath": "Other.csproj", "projectName": "Other", "requestedTfm": "net9.0"});
    let bad_site = occurrence_site(&unknown_identity, Some(&"d".repeat(40)));
    let bad = bytes(&candidate_with_occurrences(&compilations, vec![bad_site]));
    let err = admit_and_publish(&dir, &bad, &expected).unwrap_err();
    assert!(matches!(
        err,
        PublishError::Refused(RefusalReason::OccurrenceCompilationUnknown)
    ));

    let after = fs::read(compiler_facts_json_path(&dir)).unwrap();
    assert_eq!(
        before, after,
        "a refused occurrence payload must never replace the artifact"
    );
}

#[test]
fn occurrence_payload_round_trips_losslessly_including_distinct_same_line_overloads() {
    let compilations = valid_compilations();
    let site_a = occurrence_site_with_signature(
        &compilations[0].identity,
        compilations[0].fingerprint.as_deref(),
        "()->void",
    );
    let site_b = occurrence_site_with_signature(
        &compilations[0].identity,
        compilations[0].fingerprint.as_deref(),
        "(bool)->void",
    );
    let candidate = candidate_with_occurrences(&compilations, vec![site_a.clone(), site_b.clone()]);
    let facts = admit(&bytes(&candidate), &valid_expectations()).unwrap();

    let read_back: Value = serde_json::from_slice(&facts.bytes).unwrap();
    let sites = read_back["occurrences"]["sites"].as_array().unwrap();
    assert_eq!(
        sites.len(),
        2,
        "both same-line occurrences survive, not deduplicated"
    );
    assert_ne!(sites[0], sites[1], "distinct records, not merged");
    assert_eq!(
        sites[0], site_a,
        "every field survives production, admission and read-back"
    );
    assert_eq!(sites[1], site_b);
}

#[test]
fn admission_succeeds_against_a_multi_compilation_envelope_including_an_unsupported_entry() {
    // The real shape a build-context envelope carries: a top-level
    // `compilations` list with no top-level `fingerprint`, one entry
    // `unsupported` (a `null` fingerprint) -- the exact shape the pre-delta
    // self-referential check could never have admitted at all.
    let compilations = vec![
        CompilationRef {
            identity: json!({"projectPath": "Api.csproj", "projectName": "Api", "requestedTfm": "net9.0"}),
            fingerprint: Some("a".repeat(40)),
        },
        CompilationRef {
            identity: json!({"projectPath": "Legacy.csproj", "projectName": "Legacy", "requestedTfm": "net472"}),
            fingerprint: None,
        },
    ];
    let candidate = candidate_with_compilations(&compilations);
    let facts = admit(&bytes(&candidate), &valid_expectations()).unwrap();
    assert!(facts.coverage().is_complete());
}

#[test]
fn only_the_target_document_identity_differs_between_two_otherwise_identical_admitted_artifacts() {
    let compilations = valid_compilations();
    let mut site_a = occurrence_site(
        &compilations[0].identity,
        compilations[0].fingerprint.as_deref(),
    );
    site_a["targetDocumentContentIdentities"] =
        json!(["sha1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]);
    let mut site_b = site_a.clone();
    site_b["targetDocumentContentIdentities"] =
        json!(["sha1:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"]);

    let candidate_a = candidate_with_occurrences(&compilations, vec![site_a]);
    let candidate_b = candidate_with_occurrences(&compilations, vec![site_b]);

    let facts_a = admit(&bytes(&candidate_a), &valid_expectations()).unwrap();
    let facts_b = admit(&bytes(&candidate_b), &valid_expectations()).unwrap();
    assert_ne!(facts_a.bytes, facts_b.bytes);

    let doc_a: Value = serde_json::from_slice(&facts_a.bytes).unwrap();
    let doc_b: Value = serde_json::from_slice(&facts_b.bytes).unwrap();
    assert_ne!(
        doc_a["occurrences"]["sites"][0]["targetDocumentContentIdentities"],
        doc_b["occurrences"]["sites"][0]["targetDocumentContentIdentities"],
        "the changed referenced declaration's identity moves"
    );
    assert_eq!(
        doc_a["occurrences"]["sites"][0]["documentContentIdentity"],
        doc_b["occurrences"]["sites"][0]["documentContentIdentity"],
        "the consuming document's own identity does not"
    );
    assert_eq!(
        doc_a["sourceSnapshot"], doc_b["sourceSnapshot"],
        "the repository head does not move either"
    );
}
