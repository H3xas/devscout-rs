use super::*;

// --- precise TP --------------------------------------------------------

#[test]
fn precise_edge_at_a_matching_in_solution_site_scores_as_a_true_positive() {
    let files = [
        (
            "Other/MessageUrn.cs",
            "namespace App.Consumers { public static class MessageUrn { public static string Prefix { get; } } }",
        ),
        (
            "Consumers/UsesProperty.cs",
            "\nnamespace App.Consumers;\n\npublic class UsesProperty\n{\n  public object Get() => MessageUrn.Prefix;\n}\n",
        ),
    ];
    let value = graph_value_for(&files);
    let (graph_defs, edges) = parse_graph(&value).expect("graph.json parses");
    assert_eq!(edges.len(), 1, "exactly one uses-member edge expected");
    assert_eq!(edges[0].tier, Tier::Precise);

    let record = oracle_ref(
        &edges[0].from_file,
        edges[0].from_line,
        "access",
        "ident",
        edges[0].member.as_deref().unwrap_or(""),
        Some(&edges[0].to),
        Some("class"),
        false,
    );
    let universe: HashSet<String> = [edges[0].from_file.clone()].into_iter().collect();
    let report = score(Inputs {
        root: PathBuf::from("/repo"),
        graph_defs,
        oracle_defs: Vec::new(),
        edges,
        records: vec![record],
        units: Vec::new(),
        universe,
        lane: "syntax",
        discovered_shapes: HashMap::new(),
        collect_fp_sites: false,
    });

    assert_eq!(report.tiers.len(), 1);
    let (tier, ts) = &report.tiers[0];
    assert_eq!(*tier, Tier::Precise);
    assert_eq!(ts.tp, 1);
    assert_eq!(ts.fp, 0);
    assert_eq!(report.oracle_dropped, 0);
}

// --- guess FP at an external site (leak) --------------------------------

#[test]
fn guessed_edge_at_an_all_external_site_scores_as_a_leaked_false_positive() {
    let files = [
        (
            "Other/Widget.cs",
            "namespace App.Other { public class Widget { public void Frob() { } } }",
        ),
        (
            "Consumers/Guess.cs",
            "\nnamespace App.Consumers;\n\npublic class Guess\n{\n  public void Unknown()\n  {\n    var w = Compute();\n    w.Frob();\n  }\n  private object Compute() => null;\n}\n",
        ),
    ];
    let value = graph_value_for(&files);
    let (graph_defs, edges) = parse_graph(&value).expect("graph.json parses");
    let heuristic_count = edges.iter().filter(|e| e.tier == Tier::Guess).count();
    assert_eq!(heuristic_count, 1, "expected exactly one guessed edge");
    let g_edge = edges.iter().find(|e| e.tier == Tier::Guess).unwrap();

    // The oracle saw a genuinely external member at this same site (e.g.
    // an extension method from a package devscout never indexed) --
    // external, so no guessed target can ever be a true positive here.
    let record = oracle_ref(
        &g_edge.from_file,
        g_edge.from_line,
        "access",
        "ident",
        g_edge.member.as_deref().unwrap_or("Frob"),
        Some("Some.External.Type"),
        Some("class"),
        true,
    );
    let universe: HashSet<String> = [g_edge.from_file.clone()].into_iter().collect();
    let report = score(Inputs {
        root: PathBuf::from("/repo"),
        graph_defs,
        oracle_defs: Vec::new(),
        edges,
        records: vec![record],
        units: Vec::new(),
        universe,
        lane: "syntax",
        discovered_shapes: HashMap::new(),
        collect_fp_sites: false,
    });

    assert_eq!(report.tiers.len(), 1);
    let (tier, ts) = &report.tiers[0];
    assert_eq!(*tier, Tier::Guess);
    assert_eq!(ts.tp, 0);
    assert_eq!(ts.fp, 1);
    assert_eq!(ts.fp_external_site, 1);
    assert_eq!(report.silent_leak, 1);
    assert_eq!(report.silent_correct, 0);
    assert_eq!(report.oracle_external_sites, 1);
}

// --- any-match with two records on one site -----------------------------

#[test]
fn a_matching_record_among_several_at_one_site_still_earns_a_true_positive() {
    // No `member` on the edge (the legacy, schema-1 shape) -- unconstrained
    // by the member-join refinement, so this test still exercises pure
    // target-matching across several records at one site, byte-identical
    // to before that refinement existed.
    let edge = EdgeRow {
        from_file: "F.cs".into(),
        from_line: 5,
        to: "Ns.Right".into(),
        to_file: "F.cs".into(),
        tier: Tier::Precise,
        member: None,
    };
    let wrong = oracle_ref(
        "F.cs",
        5,
        "access",
        "ident",
        "M",
        Some("Ns.Wrong"),
        Some("class"),
        false,
    );
    let right = oracle_ref(
        "F.cs",
        5,
        "access",
        "ident",
        "M",
        Some("Ns.Right"),
        Some("class"),
        false,
    );
    let report = score(Inputs {
        root: PathBuf::from("/repo"),
        graph_defs: vec![DefRow {
            id: "Ns.Right".into(),
            file: "F.cs".into(),
            kind: "class".into(),
            test: false,
        }],
        oracle_defs: Vec::new(),
        edges: vec![edge],
        records: vec![wrong, right],
        units: Vec::new(),
        universe: ["F.cs".to_string()].into_iter().collect(),
        lane: "syntax",
        discovered_shapes: HashMap::new(),
        collect_fp_sites: false,
    });
    let (_, ts) = &report.tiers[0];
    assert_eq!(ts.tp, 1);
    assert_eq!(ts.fp, 0);
}

// --- member equality picks the right record at a shared site -----------

/// The motivating case for the member-join refinement: a chain-tail call
/// (`Order.Load("x").Validate()`, `tests/App.Tests/WorkerTests.cs` in
/// the fixture) puts TWO oracle records on one `(file, startLine)` --
/// `Load` on the `Order` qualifier, `Validate` on the chain's tail -- but
/// devscout's own extractor only ever emits an edge for the qualifier
/// call. Before the member-join refinement, `Validate`'s record still
/// counted as a recall HIT, because `target_matches` alone can't tell
/// the two references on that line apart: both target `Ns.Order`. With
/// member equality required, only the `Load` edge's own record vouches
/// for it, and `Validate` -- which no edge actually names -- is
/// correctly a miss.
#[test]
fn member_equality_picks_the_right_record_at_a_shared_site_chain_tail_vs_qualifier() {
    let edge = EdgeRow {
        from_file: "F.cs".into(),
        from_line: 5,
        to: "Ns.Order".into(),
        to_file: "F.cs".into(),
        tier: Tier::Precise,
        member: Some("Load".to_string()),
    };
    let load_record = oracle_ref(
        "F.cs",
        5,
        "access",
        "ident",
        "Load",
        Some("Ns.Order"),
        Some("class"),
        false,
    );
    let validate_record = oracle_ref(
        "F.cs",
        5,
        "access",
        "call",
        "Validate",
        Some("Ns.Order"),
        Some("class"),
        false,
    );
    let report = score(Inputs {
        root: PathBuf::from("/repo"),
        graph_defs: vec![DefRow {
            id: "Ns.Order".into(),
            file: "F.cs".into(),
            kind: "class".into(),
            test: false,
        }],
        oracle_defs: Vec::new(),
        edges: vec![edge],
        records: vec![load_record, validate_record],
        units: Vec::new(),
        universe: ["F.cs".to_string()].into_iter().collect(),
        lane: "syntax",
        discovered_shapes: HashMap::new(),
        collect_fp_sites: false,
    });

    // The edge itself: a TP against `load_record` only.
    let (_, ts) = &report.tiers[0];
    assert_eq!(ts.tp, 1);
    assert_eq!(ts.fp, 0);

    // Recall: both records are D-eligible, but only `load_record` is a
    // hit -- `validate_record` is a genuine miss, not a false hit
    // borrowed from the `Load` edge sharing its site.
    assert_eq!(report.recall_denominator, 2);
    assert_eq!(report.recall_all, 1);
    assert_eq!(report.top_missed, vec![("Ns.Order".to_string(), 1)]);
}

// --- member-scoped external-site vs wrong-target classification --------

/// The fixture's `entity.Property(e => e.Name)` case
/// (`src/App/AppDbContext.cs`): a guessed `Property(...)` edge landing
/// on the wrong in-tree class shares its source line with an UNRELATED
/// non-external record for a different member (`e.Name`'s lambda
/// parameter access) -- before the member-join refinement, that
/// unrelated record's mere presence at the site was enough to call the
/// guess `fp_wrong_target` instead of the external-API leak it actually
/// is (EF's real `Property(...)` fluent method, external). Scoping the
/// external-vs-wrong split to records sharing the edge's own member
/// fixes this: `e.Name` (member `Name`) no longer speaks for a `Property`
/// edge.
#[test]
fn a_different_member_non_external_record_does_not_block_an_external_leak_classification() {
    let edge = EdgeRow {
        from_file: "F.cs".into(),
        from_line: 24,
        to: "Ns.FilterConfig".into(),
        to_file: "F.cs".into(),
        tier: Tier::Guess,
        member: Some("Property".to_string()),
    };
    // The chain's outer call -- external, same member as the edge.
    let has_max_length = oracle_ref(
        "F.cs",
        24,
        "access",
        "call",
        "HasMaxLength",
        Some("Ext.PropertyBuilder"),
        Some("class"),
        true,
    );
    // The lambda parameter access `e.Name` -- non-external, but a
    // DIFFERENT member than the edge's own `Property`.
    let e_name = oracle_ref(
        "F.cs",
        24,
        "access",
        "ident",
        "Name",
        Some("Ns.Order"),
        Some("class"),
        false,
    );
    // The actual `entity.Property(...)` call the edge is a wrong guess
    // for -- external, same member (`Property`) as the edge.
    let entity_property = oracle_ref(
        "F.cs",
        24,
        "access",
        "ident",
        "Property",
        Some("Ext.EntityTypeBuilder"),
        Some("class"),
        true,
    );
    let report = score(Inputs {
        root: PathBuf::from("/repo"),
        graph_defs: vec![DefRow {
            id: "Ns.Order".into(),
            file: "F.cs".into(),
            kind: "class".into(),
            test: false,
        }],
        oracle_defs: Vec::new(),
        edges: vec![edge],
        records: vec![has_max_length, e_name, entity_property],
        units: Vec::new(),
        universe: ["F.cs".to_string()].into_iter().collect(),
        lane: "syntax",
        discovered_shapes: HashMap::new(),
        collect_fp_sites: false,
    });

    let (_, ts) = &report.tiers[0];
    assert_eq!(ts.fp, 1);
    assert_eq!(
        ts.fp_external_site, 1,
        "the member-scoped view sees only `entity_property`, which is external"
    );
    assert_eq!(
        ts.fp_wrong_target, 0,
        "`e_name` is non-external but names a different member, so it must not \
         count as an in-tree answer this edge got wrong"
    );
}

/// The other half of the same rule: when the member scope is EMPTY -- no
/// record at the site names the edge's member at all -- the split falls
/// back to the whole site rather than reading a vacuous "every scoped
/// record is external" off zero records. A site whose records are all
/// in-tree is not an external-API leak just because the edge invented a
/// member nobody wrote there, so it is `fp_wrong_target`.
#[test]
fn an_edge_naming_a_member_no_record_at_the_site_names_falls_back_to_the_whole_site() {
    let edge = EdgeRow {
        from_file: "F.cs".into(),
        from_line: 24,
        to: "Ns.FilterConfig".into(),
        to_file: "F.cs".into(),
        tier: Tier::Guess,
        member: Some("Nowhere".to_string()),
    };
    // The only record at line 24, and it is IN-TREE -- so the site has a
    // real answer, just not one for `Nowhere`.
    let in_tree = oracle_ref(
        "F.cs",
        24,
        "access",
        "ident",
        "Name",
        Some("Ns.Order"),
        Some("class"),
        false,
    );
    let report = score(Inputs {
        root: PathBuf::from("/repo"),
        graph_defs: vec![DefRow {
            id: "Ns.Order".into(),
            file: "F.cs".into(),
            kind: "class".into(),
            test: false,
        }],
        oracle_defs: Vec::new(),
        edges: vec![edge],
        records: vec![in_tree],
        units: Vec::new(),
        universe: ["F.cs".to_string()].into_iter().collect(),
        lane: "syntax",
        discovered_shapes: HashMap::new(),
        collect_fp_sites: false,
    });

    let (_, ts) = &report.tiers[0];
    assert_eq!(ts.fp, 1);
    assert_eq!(
        ts.fp_wrong_target, 1,
        "the site's only record is in-tree, so the edge is a wrong target, not a leak"
    );
    assert_eq!(
        ts.fp_external_site, 0,
        "an empty member scope must not be read as `every record here is external`"
    );
}

/// And the same empty-member-scope path at a site whose records are ALL
/// external: there the fallback agrees with the old vacuous answer, and
/// the edge really is a leak.
#[test]
fn an_edge_naming_an_unwritten_member_at_an_all_external_site_is_still_a_leak() {
    let edge = EdgeRow {
        from_file: "F.cs".into(),
        from_line: 24,
        to: "Ns.FilterConfig".into(),
        to_file: "F.cs".into(),
        tier: Tier::Guess,
        member: Some("Nowhere".to_string()),
    };
    let external_only = oracle_ref(
        "F.cs",
        24,
        "access",
        "call",
        "HasMaxLength",
        Some("Ext.PropertyBuilder"),
        Some("class"),
        true,
    );
    let report = score(Inputs {
        root: PathBuf::from("/repo"),
        graph_defs: vec![DefRow {
            id: "Ns.FilterConfig".into(),
            file: "F.cs".into(),
            kind: "class".into(),
            test: false,
        }],
        oracle_defs: Vec::new(),
        edges: vec![edge],
        records: vec![external_only],
        units: Vec::new(),
        universe: ["F.cs".to_string()].into_iter().collect(),
        lane: "syntax",
        discovered_shapes: HashMap::new(),
        collect_fp_sites: false,
    });

    let (_, ts) = &report.tiers[0];
    assert_eq!(ts.fp, 1);
    assert_eq!(ts.fp_external_site, 1);
    assert_eq!(ts.fp_wrong_target, 0);
}

// --- enum member, both spellings ----------------------------------------

#[test]
fn enum_member_edge_matches_both_the_full_and_the_bare_spelling() {
    let edge_full = EdgeRow {
        from_file: "F.cs".into(),
        from_line: 10,
        to: "Ns.OrderStatus.Open".into(),
        to_file: "Ns/OrderStatus.cs".into(),
        tier: Tier::Precise,
        member: Some("Open".to_string()),
    };
    let edge_bare = EdgeRow {
        from_file: "F.cs".into(),
        from_line: 20,
        to: "Ns.OrderStatus".into(),
        to_file: "Ns/OrderStatus.cs".into(),
        tier: Tier::Precise,
        member: Some("Open".to_string()),
    };
    let rec_at_full = oracle_ref(
        "F.cs",
        10,
        "access",
        "ident",
        "Open",
        Some("Ns.OrderStatus.Open"),
        Some("enum-member"),
        false,
    );
    let rec_at_bare = oracle_ref(
        "F.cs",
        20,
        "access",
        "ident",
        "Open",
        Some("Ns.OrderStatus.Open"),
        Some("enum-member"),
        false,
    );
    let report = score(Inputs {
        root: PathBuf::from("/repo"),
        graph_defs: vec![DefRow {
            id: "Ns.OrderStatus".into(),
            file: "Ns/OrderStatus.cs".into(),
            kind: "enum".into(),
            test: false,
        }],
        oracle_defs: Vec::new(),
        edges: vec![edge_full, edge_bare],
        records: vec![rec_at_full, rec_at_bare],
        units: Vec::new(),
        universe: ["F.cs".to_string()].into_iter().collect(),
        lane: "syntax",
        discovered_shapes: HashMap::new(),
        collect_fp_sites: false,
    });
    let (_, ts) = &report.tiers[0];
    assert_eq!(ts.tp, 2);
    assert_eq!(ts.fp, 0);
}

// --- legacy heuristic:true, and tier:"ext"/"guess" strings --------------

#[test]
fn tier_is_read_from_the_tier_string_when_present_else_from_legacy_heuristic_bool() {
    let text = r#"{"defs":[],"edges":[
        {"kind":"uses-member","from_file":"F.cs","from_line":1,"to":"Ns.A","to_file":"A.cs","heuristic":true},
        {"kind":"uses-member","from_file":"F.cs","from_line":2,"to":"Ns.B","to_file":"B.cs","tier":"ext"},
        {"kind":"uses-member","from_file":"F.cs","from_line":3,"to":"Ns.C","to_file":"C.cs","tier":"guess"},
        {"kind":"uses-member","from_file":"F.cs","from_line":4,"to":"Ns.D","to_file":"D.cs"},
        {"kind":"inherits","from_file":"F.cs","from_line":5,"to":"Ns.E","to_file":"E.cs"}
    ]}"#;
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    let (_, edges) = parse_graph(&value).unwrap();
    assert_eq!(
        edges.len(),
        4,
        "the 'inherits' edge is not uses-member and must be filtered out"
    );
    let tiers: Vec<Tier> = edges.iter().map(|e| e.tier).collect();
    assert_eq!(
        tiers,
        vec![Tier::Heuristic, Tier::Ext, Tier::Guess, Tier::Precise]
    );
}

// --- universe: an edge from a file outside units[].files is dropped ----

/// The MassTransit-run bug this refinement fixes (480 of 530 false
/// positives on that corpus): with `--units`, a `uses-member` edge whose
/// `from_file` belongs to a project the compiled `.sln` never listed at
/// all -- not `"ok"`, not `"failed"`, simply absent from `units.jsonl`
/// -- carries no oracle ground truth either way. Before this
/// refinement, only oracle RECORDS were dropped for falling outside
/// `universe`; nothing stopped such an edge from being scored, and
/// since no record ever shares its site, it landed as an `fp_no_site`
/// false positive on every run. `universe` = union of `files` across
/// every `"ok"` unit means "App/A.cs" is in it and "Other/B.cs" -- from
/// a project `units` never mentions -- is not, so the edge from
/// "Other/B.cs" is dropped before scoring, not counted as a false
/// positive.
#[test]
fn an_edge_from_a_file_outside_units_files_is_dropped_not_a_false_positive() {
    let edge = EdgeRow {
        from_file: "Other/B.cs".into(),
        from_line: 1,
        to: "Ns.Something".into(),
        to_file: "Other/B.cs".into(),
        tier: Tier::Heuristic,
        member: None,
    };
    let units = vec![Unit {
        name: "App".into(),
        status: "ok".into(),
        refs: vec![],
        files: vec!["App/A.cs".into()],
    }];
    let report = score(Inputs {
        root: PathBuf::from("/repo"),
        graph_defs: Vec::new(),
        oracle_defs: Vec::new(),
        edges: vec![edge],
        records: Vec::new(),
        units,
        // What `build_universe` would produce: the manifest's mapped
        // files ("App/A.cs" AND "Other/B.cs" -- devscout mapped both)
        // intersected with the union of ok units' files ("App/A.cs"
        // only), since "Other/B.cs" belongs to no unit at all.
        universe: ["App/A.cs".to_string()].into_iter().collect(),
        lane: "syntax",
        discovered_shapes: HashMap::new(),
        collect_fp_sites: false,
    });

    assert_eq!(report.edges_outside_universe, 1);
    assert!(
        report.tiers.is_empty(),
        "the dropped edge must not appear in any tier's stats, fp_no_site included: {:?}",
        report.tiers
    );
    assert_eq!(report.structural_checked, 0);
}

// --- structural: units method, and test-defs fallback -------------------

#[test]
fn structural_check_via_units_flags_an_edge_the_caller_project_cannot_reach() {
    let edge = EdgeRow {
        from_file: "App/A.cs".into(),
        from_line: 9,
        to: "Tests.Foo".into(),
        to_file: "Tests/Foo.cs".into(),
        tier: Tier::Heuristic,
        member: None,
    };
    let units = vec![
        Unit {
            name: "App".into(),
            status: "ok".into(),
            refs: vec!["Domain".into()],
            files: vec!["App/A.cs".into()],
        },
        Unit {
            name: "Domain".into(),
            status: "ok".into(),
            refs: vec![],
            files: vec![],
        },
        Unit {
            name: "Tests".into(),
            status: "ok".into(),
            refs: vec!["App".into()],
            files: vec!["Tests/Foo.cs".into()],
        },
    ];
    // Also a genuine (non-external) match at the site: App can never
    // reach Tests, so this edge is structurally impossible EVEN THOUGH
    // it is also the right answer to the oracle record at its site --
    // exactly the case `TierStats.structural`'s doc comment calls out.
    // The GLOBAL counts (`structural_checked`/`structural_impossible`)
    // still see it: they come from `is_structural` alone, independent
    // of TP/FP. Only the per-tier `structural` column, which feeds a
    // "these are resolver false positives" reading, excludes it.
    let record = oracle_ref(
        "App/A.cs",
        9,
        "access",
        "ident",
        "Foo",
        Some("Tests.Foo"),
        Some("class"),
        false,
    );
    let report = score(Inputs {
        root: PathBuf::from("/repo"),
        graph_defs: vec![DefRow {
            id: "Tests.Foo".into(),
            file: "Tests/Foo.cs".into(),
            kind: "class".into(),
            test: false,
        }],
        oracle_defs: Vec::new(),
        edges: vec![edge],
        records: vec![record],
        units,
        universe: ["App/A.cs".to_string()].into_iter().collect(),
        lane: "syntax",
        discovered_shapes: HashMap::new(),
        collect_fp_sites: false,
    });
    assert_eq!(report.structural_method, "units");
    assert_eq!(report.structural_checked, 1);
    assert_eq!(report.structural_impossible, 1);
    let (_, ts) = &report.tiers[0];
    assert_eq!(ts.tp, 1, "the structurally-impossible edge is still a TP");
    assert_eq!(
        ts.structural, 0,
        "a TP is never counted structural, however structurally impossible its edge -- \
         the tier column reports resolver false positives, not every structural oddity"
    );
}

#[test]
fn structural_fallback_flags_a_non_test_caller_reaching_a_test_attributed_def() {
    let edge_bad = EdgeRow {
        from_file: "App/A.cs".into(),
        from_line: 3,
        to: "Tests.Helper".into(),
        to_file: "Tests/Helper.cs".into(),
        tier: Tier::Heuristic,
        member: None,
    };
    // A second edge from a file that DOES declare a test-attributed def
    // of its own -- the fallback's second clause ("from_file has no
    // test def") should clear this one.
    let edge_ok = EdgeRow {
        from_file: "Tests/Caller.cs".into(),
        from_line: 4,
        to: "Tests.Helper".into(),
        to_file: "Tests/Helper.cs".into(),
        tier: Tier::Heuristic,
        member: None,
    };
    let report = score(Inputs {
        root: PathBuf::from("/repo"),
        graph_defs: vec![
            DefRow {
                id: "Tests.Helper".into(),
                file: "Tests/Helper.cs".into(),
                kind: "class".into(),
                test: true,
            },
            DefRow {
                id: "Tests.Caller".into(),
                file: "Tests/Caller.cs".into(),
                kind: "class".into(),
                test: true,
            },
        ],
        oracle_defs: Vec::new(),
        edges: vec![edge_bad, edge_ok],
        records: Vec::new(),
        units: Vec::new(), // empty -> "test-defs" fallback
        universe: HashSet::new(),
        lane: "syntax",
        discovered_shapes: HashMap::new(),
        collect_fp_sites: false,
    });
    assert_eq!(report.structural_method, "test-defs");
    assert_eq!(report.structural_checked, 2);
    assert_eq!(report.structural_impossible, 1);
}

/// The oracle's `--defs` view layers over the graph's for the FILE set as
/// well as the def set: a def the graph called test-attributed and the
/// oracle calls ordinary must stop making its file a test file. Otherwise
/// the caller keeps a clearance the merged view has withdrawn, and an edge
/// into a test-only def goes on being scored as legitimate.
#[test]
fn an_oracle_def_row_saying_test_false_un_marks_the_file_for_the_structural_fallback() {
    let inputs = |oracle_defs: Vec<DefRow>| Inputs {
        root: PathBuf::from("/repo"),
        graph_defs: vec![
            DefRow {
                id: "Tests.Helper".into(),
                file: "Tests/Helper.cs".into(),
                kind: "class".into(),
                test: true,
            },
            // The graph's own attribute scan called this a test def...
            DefRow {
                id: "Tests.Caller".into(),
                file: "Tests/Caller.cs".into(),
                kind: "class".into(),
                test: true,
            },
        ],
        oracle_defs,
        edges: vec![EdgeRow {
            from_file: "Tests/Caller.cs".into(),
            from_line: 4,
            to: "Tests.Helper".into(),
            to_file: "Tests/Helper.cs".into(),
            tier: Tier::Heuristic,
            member: None,
        }],
        records: Vec::new(),
        units: Vec::new(), // empty -> "test-defs" fallback
        universe: HashSet::new(),
        lane: "syntax",
        discovered_shapes: HashMap::new(),
        collect_fp_sites: false,
    };

    let without_oracle = score(inputs(Vec::new()));
    assert_eq!(
        without_oracle.structural_impossible, 0,
        "with only the graph's view the caller declares a test def, so the edge is fine"
    );

    // ...and the oracle overrules it.
    let with_oracle = score(inputs(vec![DefRow {
        id: "Tests.Caller".into(),
        file: "Tests/Caller.cs".into(),
        kind: "class".into(),
        test: false,
    }]));
    assert_eq!(with_oracle.structural_checked, 1);
    assert_eq!(
        with_oracle.structural_impossible, 1,
        "the merged view has no test def in Tests/Caller.cs any more, so the file is not a test file and the edge into a test-only def is impossible"
    );
}
