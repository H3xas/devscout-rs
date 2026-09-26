use super::*;

// Byte literals on purpose: `audit` and CI jobs read these exact keys.

#[test]
fn an_applied_record_serializes_as_its_state_alone() {
    let json = serde_json::to_string(&LaneStats::applied()).unwrap();
    assert_eq!(json, r#"{"state":"applied"}"#);
}

#[test]
fn a_not_applied_record_carries_reason_then_detail() {
    let stale = LaneStats::not_applied(
        LaneReason::StaleArtifact,
        Some("source-head-mismatch".to_string()),
    );
    assert_eq!(
        serde_json::to_string(&stale).unwrap(),
        r#"{"state":"not-applied","reason":"stale-artifact","detail":"source-head-mismatch"}"#
    );
    let no_semantic = LaneStats::not_applied(LaneReason::NoSemantic, None);
    assert_eq!(
        serde_json::to_string(&no_semantic).unwrap(),
        r#"{"state":"not-applied","reason":"no-semantic"}"#
    );
}

#[test]
fn every_reason_has_its_documented_token() {
    let tokens: Vec<&str> = [
        LaneReason::StaleArtifact,
        LaneReason::ImportsMoved,
        LaneReason::ArtifactUnreadable,
        LaneReason::NoSemantic,
    ]
    .iter()
    .map(|r| r.as_str())
    .collect();
    assert_eq!(
        tokens,
        [
            "stale-artifact",
            "imports-moved",
            "artifact-unreadable",
            "no-semantic"
        ]
    );
}

#[test]
fn a_state_this_build_does_not_know_still_reads_back() {
    let read: LaneStats = serde_json::from_str(r#"{"state":"partial","extra":1}"#).unwrap();
    assert_eq!(read.state(), "partial");
    assert!(!read.is_applied());
    assert_eq!(read.reason(), None);
    assert_eq!(read.detail(), None);
}

#[test]
fn the_text_line_names_the_lane_and_why_it_did_not_apply() {
    assert_eq!(LaneStats::applied().line(), "lane: enriched");
    assert_eq!(
        LaneStats::not_applied(LaneReason::ArtifactUnreadable, None).line(),
        "lane: syntax (enriched lane not applied: artifact-unreadable)"
    );
    assert_eq!(
        LaneStats::not_applied(LaneReason::NoSemantic, None).line(),
        "lane: syntax (enriched lane not applied: no-semantic)"
    );
    assert_eq!(
        LaneStats::not_applied(
            LaneReason::StaleArtifact,
            Some("working-tree-dirty".to_string())
        )
        .line(),
        "lane: syntax (enriched lane not applied: stale-artifact: working-tree-dirty)"
    );
    // A detail that itself contains the separator is carried verbatim.
    assert_eq!(
        LaneStats::not_applied(
            LaneReason::ImportsMoved,
            Some("import-file-changed:external-file:../x.props".to_string())
        )
        .line(),
        "lane: syntax (enriched lane not applied: imports-moved: import-file-changed:external-file:../x.props)"
    );
}

#[test]
fn an_unknown_state_without_a_reason_is_named_as_it_is() {
    let read: LaneStats = serde_json::from_str(r#"{"state":"partial"}"#).unwrap();
    assert_eq!(read.line(), "lane: partial");
}
