// The lane `audit` reads off graph.json and what it prints for it: the label,
// the text header, the `lane_detail`/`compilations` keys, and `--assert`
// over the compilation counts. Graph values are written out by hand, so each
// case states the exact `stats` block it judges.
use serde_json::{json, Value};

use super::*;

fn graph(stats: Value) -> Value {
    json!({"defs": [], "edges": [], "stats": stats})
}

fn report_for(stats: Value) -> AuditReport {
    score(Inputs {
        root: PathBuf::from("/repo"),
        graph_defs: Vec::new(),
        oracle_defs: Vec::new(),
        edges: Vec::new(),
        records: Vec::new(),
        units: Vec::new(),
        universe: HashSet::new(),
        lane: AuditLane::read(&graph(stats)),
        discovered_shapes: HashMap::new(),
        collect_fp_sites: false,
    })
}

fn json_report(stats: Value) -> Value {
    serde_json::from_str(&render_json(&report_for(stats))).expect("report is JSON")
}

fn header(stats: Value) -> String {
    render_text(&report_for(stats))
        .lines()
        .next()
        .expect("header line")
        .to_string()
}

fn semantic() -> Value {
    json!({"confirmed": 16, "disagreements": 0, "discovered": 15})
}

fn partial_record() -> Value {
    json!({
        "state": "partial",
        "reason": "compilations-incomplete",
        "detail": "3 of 6 compilations partial: workspace-failure 3",
        "compilations": {"total": 6, "partial": 3},
    })
}

#[test]
fn a_partial_record_reads_enriched_partial_with_its_detail_in_the_header() {
    let stats = json!({"semantic": semantic(), "lane": partial_record()});
    let report = json_report(stats.clone());
    assert_eq!(report["lane"], "enriched-partial");
    assert_eq!(report["lane_detail"], partial_record());
    assert_eq!(report["compilations"], json!({"total": 6, "partial": 3}));
    assert!(
        header(stats).contains(
            "  lane enriched-partial (3 of 6 compilations partial: workspace-failure 3)  "
        ),
        "the header names the partial compilations"
    );
}

#[test]
fn an_applied_record_reads_enriched_with_an_unchanged_header() {
    let record = json!({"state": "applied", "compilations": {"total": 6, "partial": 0}});
    let stats = json!({"semantic": semantic(), "lane": record});
    let report = json_report(stats.clone());
    assert_eq!(report["lane"], "enriched");
    assert_eq!(report["lane_detail"], record);
    assert_eq!(report["compilations"], json!({"total": 6, "partial": 0}));
    assert!(header(stats).contains("  lane enriched  oracle "));
}

#[test]
fn every_not_applied_reason_reads_syntax_with_the_reason_in_the_header() {
    let cases = [
        (
            json!({"state": "not-applied", "reason": "stale-artifact", "detail": "working-tree-dirty"}),
            "lane syntax (enriched lane not applied: stale-artifact: working-tree-dirty)",
        ),
        (
            json!({"state": "not-applied", "reason": "imports-moved", "detail": "import-file-missing:external-file:../a.props"}),
            "lane syntax (enriched lane not applied: imports-moved: import-file-missing:external-file:../a.props)",
        ),
        (
            json!({"state": "not-applied", "reason": "artifact-unreadable"}),
            "lane syntax (enriched lane not applied: artifact-unreadable)",
        ),
        (
            json!({"state": "not-applied", "reason": "no-semantic"}),
            "lane syntax (enriched lane not applied: no-semantic)",
        ),
        (
            json!({"state": "not-applied", "reason": "no-complete-compilation", "detail": "6 of 6 compilations partial: workspace-failure 6", "compilations": {"total": 6, "partial": 6}}),
            "lane syntax (enriched lane not applied: no-complete-compilation: 6 of 6 compilations partial: workspace-failure 6)",
        ),
        (
            json!({"state": "not-applied", "reason": "nothing-confirmed", "compilations": {"total": 6, "partial": 0}}),
            "lane syntax (enriched lane not applied: nothing-confirmed)",
        ),
    ];
    for (record, token) in cases {
        let stats = json!({"lane": record});
        let report = json_report(stats.clone());
        assert_eq!(report["lane"], "syntax", "{record}");
        assert_eq!(report["lane_detail"], record);
        assert_eq!(
            report.get("compilations"),
            record.get("compilations"),
            "{record}"
        );
        assert!(header(stats).contains(&format!("  {token}  ")), "{record}");
    }
}

#[test]
fn a_graph_without_a_lane_record_reads_as_before_and_gains_no_key() {
    for (stats, lane) in [
        (json!({"semantic": semantic()}), "enriched"),
        (json!({}), "syntax"),
    ] {
        let text = render_json(&report_for(stats.clone()));
        let report: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(report["lane"], lane);
        assert!(report.get("lane_detail").is_none(), "{text}");
        assert!(report.get("compilations").is_none(), "{text}");
        assert!(text.ends_with(r#","edges_outside_universe":0}"#), "{text}");
        assert!(header(stats).contains(&format!("  lane {lane}  oracle ")));
    }
}

#[test]
fn a_state_this_build_does_not_know_keeps_the_semantic_reading() {
    let stats =
        json!({"semantic": semantic(), "lane": {"state": "reconsidered", "reason": "future"}});
    assert_eq!(json_report(stats.clone())["lane"], "enriched");
    assert!(header(stats).contains("  lane enriched  oracle "));
}

#[test]
fn the_lane_keys_follow_every_existing_key() {
    let text = render_json(&report_for(
        json!({"semantic": semantic(), "lane": partial_record()}),
    ));
    let tail = concat!(
        r#","edges_outside_universe":0,"lane_detail":{"compilations":{"partial":3,"total":6},"#,
        r#""detail":"3 of 6 compilations partial: workspace-failure 3","#,
        r#""reason":"compilations-incomplete","state":"partial"},"#,
        r#""compilations":{"partial":3,"total":6}}"#
    );
    assert!(text.ends_with(tail), "{text}");
}

#[test]
fn assert_reads_the_compilation_counts_and_reports_them_missing_without_a_record() {
    let pin = r#"{"compilations.partial": {"max": 0}, "compilations.total": {"min": 1}}"#;

    let degraded = render_json(&report_for(
        json!({"semantic": semantic(), "lane": partial_record()}),
    ));
    assert_eq!(
        evaluate_assert(&degraded, pin).unwrap(),
        vec!["assert: compilations.partial = 3 > max 0"]
    );

    let healthy = render_json(&report_for(json!({
        "semantic": semantic(),
        "lane": {"state": "applied", "compilations": {"total": 6, "partial": 0}},
    })));
    assert!(evaluate_assert(&healthy, pin).unwrap().is_empty());

    let before_the_record = render_json(&report_for(json!({"semantic": semantic()})));
    assert_eq!(
        evaluate_assert(&before_the_record, pin).unwrap(),
        vec![
            "assert: compilations.partial missing",
            "assert: compilations.total missing"
        ]
    );
}
