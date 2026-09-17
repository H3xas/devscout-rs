use super::*;

#[test]
fn refs_json_leads_with_schema_version_ahead_of_every_existing_key() {
    let model = json_refs_model(Vec::new(), false);
    let json = refs_model_to_json(&model);
    assert!(
        json.starts_with(
            r#"{"schema_version":1,"status":"resolved","query":"Widget","id":"App.Widget","kind":"class""#
        ),
        "schema_version is the first key and every existing key keeps its slot after it: {json}"
    );
}

#[test]
fn refs_json_appends_heuristic_then_source_last_and_omits_each_when_it_has_no_value() {
    let model = json_refs_model(
        vec![
            query::InboundRow {
                file: "src/Fact.cs".into(),
                line: 4,
                heuristic: false,
                tier: None,
                source: String::new(),
                occurrence_index: None,
            },
            query::InboundRow {
                file: "src/Guess.cs".into(),
                line: 9,
                heuristic: true,
                tier: None,
                source: "var w = new Widget();".into(),
                occurrence_index: None,
            },
        ],
        true,
    );
    let json = refs_model_to_json(&model);
    assert!(
        json.contains(
            r#""rows":[{"file":"src/Fact.cs","line":4,"why":"uses-member-precise"},{"file":"src/Guess.cs","line":9,"heuristic":true,"source":"var w = new Widget();","why":"uses-member-guess"}]"#
        ),
        "{json}"
    );
}

#[test]
fn refs_json_appends_tier_after_heuristic_and_omits_it_on_a_precise_row() {
    let row = |file: &str, line: usize, tier: Option<graph::HeuristicTier>| query::InboundRow {
        file: file.into(),
        line,
        heuristic: tier.is_some(),
        tier,
        source: String::new(),
        occurrence_index: None,
    };
    let model = json_refs_model(
        vec![
            row("src/Fact.cs", 4, None),
            row("src/Ext.cs", 7, Some(graph::HeuristicTier::Ext)),
            row("src/Guess.cs", 9, Some(graph::HeuristicTier::Guess)),
        ],
        true,
    );
    let json = refs_model_to_json(&model);
    // A precise row is byte-identical to what it was before the tier
    // existed; a guessed one gains exactly one key, in the slot right after
    // the flag it refines and still before `source`.
    assert!(
        json.contains(
            r#""rows":[{"file":"src/Fact.cs","line":4,"why":"uses-member-precise"},{"file":"src/Ext.cs","line":7,"heuristic":true,"tier":"ext","why":"uses-member-ext"},{"file":"src/Guess.cs","line":9,"heuristic":true,"tier":"guess","why":"uses-member-guess"}]"#
        ),
        "{json}"
    );
    assert_eq!(
        json.matches(r#""tier""#).count(),
        2,
        "a precise row carries no trace of the key: {json}"
    );
}

#[test]
fn refs_json_omits_the_outbound_key_entirely_without_out_and_keeps_its_slot_with_it() {
    let row = || {
        vec![query::InboundRow {
            file: "src/Fact.cs".into(),
            line: 4,
            heuristic: false,
            tier: None,
            source: String::new(),
            occurrence_index: None,
        }]
    };
    let without = refs_model_to_json(&json_refs_model(row(), false));
    assert!(
        !without.contains(r#""outbound":{"inherits""#),
        "the default model must carry no outbound tables: {without}"
    );
    assert!(without.contains(r#""ambiguous":{"inbound""#), "{without}");

    let with = refs_model_to_json(&json_refs_model(row(), true));
    let outbound_at = with
        .find(r#""outbound":{"inherits""#)
        .expect("--out must emit the outbound tables");
    let inbound_at = with
        .find(r#""inbound":{"inherits""#)
        .expect("inbound is always emitted");
    let ambiguous_at = with
        .find(r#""ambiguous":{"inbound""#)
        .expect("ambiguous is always emitted");
    assert!(
        inbound_at < outbound_at && outbound_at < ambiguous_at,
        "outbound keeps JS's key slot: {with}"
    );
}

#[test]
fn member_refs_json_wraps_unchanged_resolved_models_under_status_query_members() {
    let row = || {
        vec![query::InboundRow {
            file: "src/Fact.cs".into(),
            line: 4,
            heuristic: false,
            tier: None,
            source: String::new(),
            occurrence_index: None,
        }]
    };
    let one = json_refs_model(row(), false);
    let json = member_refs_to_json("Widget", std::slice::from_ref(&one), query::Outcome::Hit);
    assert!(
        json.starts_with(
            r#"{"schema_version":1,"status":"members","query":"Widget","members":[{"schema_version":1,"status":"resolved""#
        ),
        "schema_version is the first key, ahead of status, on both the wrapper and each member: {json}"
    );
    assert!(
        json.ends_with(r#"}],"outcome":"hit"}"#),
        "the wrapper's own outcome is appended last, after the members array: {json}"
    );
    assert!(
        json.contains(&refs_model_to_json(&one)),
        "a member entry is the resolved object unchanged: {json}"
    );
}
