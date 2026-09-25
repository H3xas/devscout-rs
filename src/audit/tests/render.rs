use super::*;

// --- --assert: pass, fail, missing path ----------------------------------

#[test]
fn assert_thresholds_report_pass_fail_and_missing_path_distinctly() {
    let report_json = r#"{"tiers":{"guess":{"fp":13}},"recall":{"all":0.867}}"#;

    let pass = evaluate_assert(report_json, r#"{"recall.all":{"min":0.8}}"#).unwrap();
    assert!(pass.is_empty());

    let fail = evaluate_assert(report_json, r#"{"tiers.guess.fp":{"max":0}}"#).unwrap();
    assert_eq!(fail, vec!["assert: tiers.guess.fp = 13 > max 0"]);

    let missing =
        evaluate_assert(report_json, r#"{"tiers.precise.precision":{"min":1.0}}"#).unwrap();
    assert_eq!(missing, vec!["assert: tiers.precise.precision missing"]);
}

// --- JSON key-order snapshot ---------------------------------------------

#[test]
fn json_output_key_order_is_pinned() {
    let report = AuditReport {
        root: "/repo".to_string(),
        lane: "syntax",
        oracle_records: 34,
        oracle_sites: 25,
        oracle_external_sites: 8,
        oracle_ambiguous: 1,
        oracle_dropped: 2,
        units_ok: 6,
        units_failed: 0,
        structural_method: "units",
        tiers: vec![
            (
                Tier::Precise,
                TierStats {
                    edges: 6,
                    tp: 6,
                    fp: 0,
                    fp_no_site: 0,
                    fp_external_site: 0,
                    fp_wrong_target: 0,
                    structural: 0,
                    unjudged: 0,
                },
            ),
            (
                Tier::Heuristic,
                TierStats {
                    edges: 21,
                    tp: 8,
                    fp: 13,
                    fp_no_site: 0,
                    fp_external_site: 13,
                    fp_wrong_target: 0,
                    structural: 2,
                    unjudged: 0,
                },
            ),
        ],
        recall_denominator: 15,
        recall_precise: 6,
        recall_precise_ext: 7,
        recall_all: 13,
        by_receiver: vec![
            ("ident", Some(0.917)),
            ("qualified", None),
            ("this", Some(0.0)),
            ("base", None),
            ("call", Some(0.0)),
        ],
        recall_conditional: 1,
        recall_bare: 0,
        silent_correct: 4,
        silent_leak: 4,
        structural_impossible: 2,
        structural_checked: 21,
        fanout: [20, 4, 0, 0],
        unknown_targets: vec![("class".to_string(), 2)],
        top_fp: vec![("FilterConfig".to_string(), 2), ("Mailer".to_string(), 1)],
        top_missed: vec![("Fixture.Domain.Order".to_string(), 2)],
        partial_file_mismatch: 0,
        edges_outside_universe: 5,
        fp_sites: Vec::new(),
    };
    let json = render_json(&report);
    assert_eq!(
        json,
        concat!(
            "{\"status\":\"ok\",\"root\":\"/repo\",\"lane\":\"syntax\",",
            "\"oracle\":{\"records\":34,\"sites\":25,\"external_sites\":8,\"ambiguous\":1,\"dropped\":2},",
            "\"units\":{\"ok\":6,\"failed\":0,\"method\":\"units\"},",
            "\"tiers\":{",
            "\"precise\":{\"edges\":6,\"tp\":6,\"fp\":0,\"precision\":1.000,\"fp_no_site\":0,\"fp_external_site\":0,\"fp_wrong_target\":0,\"structural\":0,\"unjudged\":0},",
            "\"heuristic\":{\"edges\":21,\"tp\":8,\"fp\":13,\"precision\":0.381,\"fp_no_site\":0,\"fp_external_site\":13,\"fp_wrong_target\":0,\"structural\":2,\"unjudged\":0}",
            "},",
            "\"recall\":{\"denominator\":15,\"precise\":0.400,\"precise_ext\":0.467,\"all\":0.867,",
            "\"by_receiver\":{\"ident\":0.917,\"qualified\":null,\"this\":0.000,\"base\":null,\"call\":0.000},",
            "\"conditional\":1,\"bare\":0},",
            "\"silent\":{\"correct\":4,\"leak\":4},",
            "\"structural\":{\"impossible\":2,\"checked\":21,\"method\":\"units\"},",
            "\"fanout\":{\"1\":20,\"2\":4,\"3\":0,\"4+\":0},",
            "\"unknown_targets\":[{\"kind\":\"class\",\"count\":2}],",
            "\"top_fp\":[{\"name\":\"FilterConfig\",\"count\":2},{\"name\":\"Mailer\",\"count\":1}],",
            "\"top_missed\":[{\"id\":\"Fixture.Domain.Order\",\"count\":2}],",
            "\"partial_file_mismatch\":0,",
            "\"edges_outside_universe\":5}",
        )
    );
}

// --- text layout snapshot ---------------------------------------------

#[test]
fn text_output_layout_is_pinned() {
    let report = AuditReport {
        root: "/repo".to_string(),
        lane: "syntax",
        oracle_records: 34,
        oracle_sites: 25,
        oracle_external_sites: 8,
        oracle_ambiguous: 1,
        oracle_dropped: 0,
        units_ok: 6,
        units_failed: 0,
        structural_method: "units",
        tiers: vec![
            (
                Tier::Precise,
                TierStats {
                    edges: 6,
                    tp: 6,
                    fp: 0,
                    fp_no_site: 0,
                    fp_external_site: 0,
                    fp_wrong_target: 0,
                    structural: 0,
                    unjudged: 0,
                },
            ),
            (
                Tier::Guess,
                TierStats {
                    edges: 21,
                    tp: 8,
                    fp: 13,
                    fp_no_site: 0,
                    fp_external_site: 13,
                    fp_wrong_target: 0,
                    structural: 2,
                    unjudged: 0,
                },
            ),
        ],
        recall_denominator: 15,
        recall_precise: 6,
        recall_precise_ext: 7,
        recall_all: 13,
        by_receiver: vec![
            ("ident", Some(0.917)),
            ("qualified", None),
            ("this", Some(0.0)),
        ],
        recall_conditional: 1,
        recall_bare: 0,
        silent_correct: 4,
        silent_leak: 4,
        structural_impossible: 3,
        structural_checked: 21,
        fanout: [20, 4, 0, 0],
        unknown_targets: vec![("class".to_string(), 2)],
        top_fp: vec![("FilterConfig".to_string(), 2), ("Mailer".to_string(), 1)],
        top_missed: vec![("Fixture.Domain.Order".to_string(), 2)],
        partial_file_mismatch: 0,
        edges_outside_universe: 5,
        fp_sites: Vec::new(),
    };
    let text = render_text(&report);
    assert_eq!(
        text,
        concat!(
            "devscout audit --semantic  root /repo  lane syntax  oracle 34 records / 25 sites  units ok 6 failed 0  method units\n",
            "tier        edges     tp     fp   precision   fp:no-site  fp:external  fp:wrong  structural  unjudged\n",
            "precise         6      6      0       1.000            0            0         0           0         0\n",
            "guess          21      8     13       0.381            0           13         0           2         0\n",
            "recall (15 in-graph member sites)  precise 0.400  precise+ext 0.467  all 0.867\n",
            "  by receiver  ident 0.917  qualified -  this 0.000\n",
            "external sites 8  silent-correct 4  leaked 4\n",
            "structural  impossible 3  checked 21\n",
            "fan-out  1: 20  2: 4  3: 0  4+: 0\n",
            "top fp targets   FilterConfig 2  Mailer 1\n",
            "top missed       Fixture.Domain.Order 2\n",
            "unknown targets  class 2\n",
            "ambiguous 1\n",
            "edges outside universe (not judged) 5",
        )
    );
}
