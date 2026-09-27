//! Integration tests for the lane record once the admitted artifact's own
//! compilations are read: a partial lane, the two not-applied reasons that
//! come from the compilations, their precedence under a stale artifact, the
//! applied record's counts, and what `audit` reports for each.
//!
//! See `fixtures/csharp-enrichment/README.md`.

mod support;

use std::fs;

use serde_json::{json, Value};
use support::{
    candidate, compilation_identity, confirmed_load_site, lane_line, with_compilations, Fixture,
};

const COMPLETE: (&str, &str) = ("complete", "complete");
const WORKSPACE_FAILURE: (&str, &str) = ("partial", "workspace-failure");

/// The fixture's own compilation, which the occurrence sites name, followed
/// by five more distinct ones; `states` gives all six in that order.
fn six_compilations(
    states: [(&'static str, &'static str); 6],
) -> Vec<(Value, &'static str, &'static str)> {
    states
        .iter()
        .enumerate()
        .map(|(i, (state, reason))| {
            let identity = if i == 0 {
                compilation_identity()
            } else {
                json!({
                    "projectPath": format!("Extra{i}/Extra{i}.csproj"),
                    "projectName": format!("Extra{i}"),
                    "requestedTfm": "net9.0",
                    "effectiveTfm": "net9.0",
                    "configuration": "Debug",
                    "platform": "AnyCPU",
                })
            };
            (identity, *state, *reason)
        })
        .collect()
}

/// Maps once with no artifact, admits an artifact over `states` (with one
/// confirmed `Config.Load()` fact when `confirm`), and maps again. Returns the second map's stdout.
fn admit(fx: &Fixture, states: [(&'static str, &'static str); 6], confirm: bool) -> String {
    fx.map(&[]);
    let sites = if confirm {
        vec![confirmed_load_site(fx.first_load_line())]
    } else {
        vec![]
    };
    fx.import(&with_compilations(
        candidate(&fx.head(), vec![], sites),
        &six_compilations(states),
    ));
    fx.map(&[])
}

fn no_artifact_edges(prefix: &str) -> Value {
    let twin = Fixture::build(prefix);
    twin.map(&[]);
    let g = twin.graph();
    assert!(g["stats"].get("lane").is_none(), "{}", g["stats"]);
    g["edges"].clone()
}

fn audit_report(fx: &Fixture) -> Value {
    let out = fx.audit(&["--json"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    serde_json::from_slice(&out.stdout).expect("audit --json is JSON")
}

fn audit_header(fx: &Fixture) -> String {
    let out = fx.audit(&[]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_string()
}

/// `audit --json --assert` with one `compilations.partial` ceiling of 0.
fn assert_no_partial(fx: &Fixture) -> std::process::Output {
    let expectations = fx.home.join("no-partial.json");
    fs::write(&expectations, r#"{"compilations.partial": {"max": 0}}"#).unwrap();
    fx.audit(&["--json", "--assert", expectations.to_str().unwrap()])
}

const PARTIAL_DETAIL: &str = "3 of 6 compilations partial: workspace-failure 3";

#[test]
fn facts_used_over_some_partial_compilations_print_and_record_the_partial_lane() {
    let fx = Fixture::build("partial");
    let stdout = admit(
        &fx,
        [
            COMPLETE,
            COMPLETE,
            COMPLETE,
            WORKSPACE_FAILURE,
            WORKSPACE_FAILURE,
            WORKSPACE_FAILURE,
        ],
        true,
    );

    assert_eq!(
        lane_line(&stdout),
        Some(format!("lane: enriched, partial ({PARTIAL_DETAIL})").as_str()),
        "{stdout}"
    );
    let g = fx.graph();
    let record = json!({
        "state": "partial",
        "reason": "compilations-incomplete",
        "detail": PARTIAL_DETAIL,
        "compilations": {"total": 6, "partial": 3},
    });
    assert_eq!(g["stats"]["lane"], record);
    assert_eq!(g["stats"]["semantic"]["confirmed"], 1, "{}", g["stats"]);

    let report = audit_report(&fx);
    assert_eq!(report["lane"], "enriched-partial");
    assert_eq!(report["lane_detail"], record);
    assert_eq!(report["compilations"], json!({"total": 6, "partial": 3}));
    assert!(
        audit_header(&fx).contains(&format!("  lane enriched-partial ({PARTIAL_DETAIL})  ")),
        "{}",
        audit_header(&fx)
    );

    let asserted = assert_no_partial(&fx);
    assert_eq!(asserted.status.code(), Some(1), "{asserted:?}");
    assert_eq!(
        String::from_utf8(asserted.stderr).unwrap(),
        "assert: compilations.partial = 3 > max 0\n"
    );
    let still_the_report: Value = serde_json::from_slice(&asserted.stdout).unwrap();
    assert_eq!(still_the_report["lane"], "enriched-partial");
}

#[test]
fn no_complete_compilation_leaves_the_syntax_edges_and_records_why() {
    let fx = Fixture::build("none-complete");
    let stdout = admit(&fx, [WORKSPACE_FAILURE; 6], true);

    let detail = "6 of 6 compilations partial: workspace-failure 6";
    assert_eq!(
        lane_line(&stdout),
        Some(
            format!("lane: syntax (enriched lane not applied: no-complete-compilation: {detail})")
                .as_str()
        ),
        "{stdout}"
    );
    let g = fx.graph();
    let record = json!({
        "state": "not-applied",
        "reason": "no-complete-compilation",
        "detail": detail,
        "compilations": {"total": 6, "partial": 6},
    });
    assert_eq!(g["stats"]["lane"], record);
    assert!(g["stats"].get("semantic").is_none(), "{}", g["stats"]);
    assert_eq!(g["edges"], no_artifact_edges("none-complete-twin"));

    let report = audit_report(&fx);
    assert_eq!(report["lane"], "syntax");
    assert_eq!(report["lane_detail"], record);
    assert_eq!(report["compilations"], json!({"total": 6, "partial": 6}));
    assert!(
        audit_header(&fx).contains(&format!(
            "  lane syntax (enriched lane not applied: no-complete-compilation: {detail})  "
        )),
        "{}",
        audit_header(&fx)
    );
}

#[test]
fn complete_compilations_that_confirm_nothing_are_not_applied() {
    let fx = Fixture::build("nothing-confirmed");
    let stdout = admit(&fx, [COMPLETE; 6], false);

    assert_eq!(
        lane_line(&stdout),
        Some("lane: syntax (enriched lane not applied: nothing-confirmed)"),
        "{stdout}"
    );
    let g = fx.graph();
    let record = json!({
        "state": "not-applied",
        "reason": "nothing-confirmed",
        "compilations": {"total": 6, "partial": 0},
    });
    assert_eq!(g["stats"]["lane"], record);
    assert!(g["stats"].get("semantic").is_none(), "{}", g["stats"]);
    assert_eq!(g["edges"], no_artifact_edges("nothing-confirmed-twin"));

    let report = audit_report(&fx);
    assert_eq!(report["lane"], "syntax");
    assert_eq!(report["lane_detail"], record);
    assert_eq!(report["compilations"], json!({"total": 6, "partial": 0}));
}

#[test]
fn nothing_confirmed_over_partial_compilations_carries_their_detail() {
    let fx = Fixture::build("nothing-confirmed-partial");
    let stdout = admit(
        &fx,
        [
            COMPLETE,
            COMPLETE,
            COMPLETE,
            WORKSPACE_FAILURE,
            WORKSPACE_FAILURE,
            WORKSPACE_FAILURE,
        ],
        false,
    );

    assert_eq!(
        lane_line(&stdout),
        Some(
            format!(
                "lane: syntax (enriched lane not applied: nothing-confirmed: {PARTIAL_DETAIL})"
            )
            .as_str()
        ),
        "{stdout}"
    );
    let g = fx.graph();
    assert_eq!(
        g["stats"]["lane"],
        json!({
            "state": "not-applied",
            "reason": "nothing-confirmed",
            "detail": PARTIAL_DETAIL,
            "compilations": {"total": 6, "partial": 3},
        })
    );
    assert!(g["stats"].get("semantic").is_none(), "{}", g["stats"]);
    assert_eq!(
        g["edges"],
        no_artifact_edges("nothing-confirmed-partial-twin")
    );
}

#[test]
fn a_stale_artifact_over_partial_compilations_is_reported_stale_without_counts() {
    let fx = Fixture::build("stale-over-partial");
    admit(&fx, [WORKSPACE_FAILURE; 6], true);
    fx.commit_unrelated_file();
    let stdout = fx.map(&[]);

    assert_eq!(
        lane_line(&stdout),
        Some("lane: syntax (enriched lane not applied: stale-artifact: source-head-mismatch)"),
        "{stdout}"
    );
    let g = fx.graph();
    assert_eq!(
        g["stats"]["lane"],
        json!({"state": "not-applied", "reason": "stale-artifact", "detail": "source-head-mismatch"})
    );
    let report = audit_report(&fx);
    assert_eq!(report["lane"], "syntax");
    assert!(report.get("compilations").is_none(), "{report}");
    assert_eq!(report["lane_detail"], g["stats"]["lane"]);
}

#[test]
fn facts_used_over_complete_compilations_record_applied_with_counts() {
    let fx = Fixture::build("applied-counts");
    let stdout = admit(&fx, [COMPLETE; 6], true);

    assert_eq!(lane_line(&stdout), Some("lane: enriched"), "{stdout}");
    let g = fx.graph();
    assert_eq!(
        g["stats"]["lane"],
        json!({"state": "applied", "compilations": {"total": 6, "partial": 0}})
    );
    assert_eq!(g["stats"]["semantic"]["confirmed"], 1, "{}", g["stats"]);

    let report = audit_report(&fx);
    assert_eq!(report["lane"], "enriched");
    assert_eq!(report["compilations"], json!({"total": 6, "partial": 0}));
    let header = audit_header(&fx);
    assert!(header.contains("  lane enriched  "), "{header}");

    let asserted = assert_no_partial(&fx);
    assert_eq!(asserted.status.code(), Some(0), "{asserted:?}");
    assert!(asserted.stderr.is_empty(), "{asserted:?}");
}

#[test]
fn a_graph_with_no_lane_record_audits_without_the_new_keys() {
    let fx = Fixture::build("no-lane-record");
    fx.map(&[]);

    let report = audit_report(&fx);
    assert_eq!(report["lane"], "syntax");
    assert!(report.get("lane_detail").is_none(), "{report}");
    assert!(report.get("compilations").is_none(), "{report}");

    let asserted = assert_no_partial(&fx);
    assert_eq!(asserted.status.code(), Some(1), "{asserted:?}");
    assert_eq!(
        String::from_utf8(asserted.stderr).unwrap(),
        "assert: compilations.partial missing\n"
    );
}
