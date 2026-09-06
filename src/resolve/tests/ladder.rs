use super::*;

#[test]
fn resolve_graph_threads_the_real_head_hash_through_built_at_head() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("scout-resolve-head-test-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
            .status()
            .unwrap()
            .success());
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    std::fs::write(dir.join("a.txt"), "hi").unwrap();
    run(&["add", "a.txt"]);
    run(&["commit", "-q", "-m", "one"]);
    let expected = manifest::git_head(&dir).expect("git_head must see the commit just made");

    let g = resolve_graph(&dir, &[]);
    assert_eq!(g.built_at_head, Some(expected));
}

#[test]
fn resolve_graph_built_at_head_is_none_outside_any_repo() {
    let g = resolve_graph(&no_git_root(), &[]);
    assert_eq!(g.built_at_head, None);
}

// --- alias short-circuit --------------------------------------------

#[test]
fn alias_wins_over_an_otherwise_ambiguous_simple_name() {
    // Two "Money" classes plus an alias pinning "Cash" to one of them.
    // A bare reference to the ALIAS NAME must resolve cleanly even
    // though "Money" itself would be globally ambiguous.
    let files = vec![
        (
            "A/Money.cs".to_string(),
            frag(vec![def("A.Money", "Money", "A", "class")], vec![], vec![]),
        ),
        (
            "B/Money.cs".to_string(),
            frag(vec![def("B.Money", "Money", "B", "class")], vec![], vec![]),
        ),
        (
            "C/Wallet.cs".to_string(),
            frag(
                vec![def("C.Wallet", "Wallet", "C", "class")],
                vec![FragUsing::Alias {
                    alias: "Cash".into(),
                    target: "A.Money".into(),
                    global: false,
                }],
                vec![type_ref("uses-type", "Cash", None, "C")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edge =
        find_edge(&g, |e| matches!(e, Edge::UsesType { .. })).expect("resolved edge present");
    match edge {
        Edge::UsesType { to, .. } => assert_eq!(to, "A.Money"),
        _ => unreachable!(),
    }
    assert_eq!(g.stats.ambiguous_count, 0);
}

#[test]
fn local_alias_shadows_a_same_named_global_alias() {
    let files = vec![
        (
            "A/Money.cs".to_string(),
            frag(vec![def("A.Money", "Money", "A", "class")], vec![], vec![]),
        ),
        (
            "B/Money.cs".to_string(),
            frag(vec![def("B.Money", "Money", "B", "class")], vec![], vec![]),
        ),
        (
            "Globals.cs".to_string(),
            frag(
                vec![],
                vec![FragUsing::Alias {
                    alias: "Cash".into(),
                    target: "A.Money".into(),
                    global: true,
                }],
                vec![],
            ),
        ),
        (
            "C/Wallet.cs".to_string(),
            frag(
                vec![def("C.Wallet", "Wallet", "C", "class")],
                vec![FragUsing::Alias {
                    alias: "Cash".into(),
                    target: "B.Money".into(),
                    global: false,
                }],
                vec![type_ref("uses-type", "Cash", None, "C")],
            ),
        ),
        // A file with NO local override sees the global alias.
        (
            "D/Ledger.cs".to_string(),
            frag(
                vec![def("D.Ledger", "Ledger", "D", "class")],
                vec![],
                vec![type_ref("uses-type", "Cash", None, "D")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let shadowed = g
        .edges
        .iter()
        .find(|e| matches!(e, Edge::UsesType { from_file, .. } if from_file == "C/Wallet.cs"))
        .expect("shadowed edge present");
    let global = g
        .edges
        .iter()
        .find(|e| matches!(e, Edge::UsesType { from_file, .. } if from_file == "D/Ledger.cs"))
        .expect("global edge present");
    match (shadowed, global) {
        (
            Edge::UsesType {
                to: shadowed_to, ..
            },
            Edge::UsesType { to: global_to, .. },
        ) => {
            assert_eq!(
                shadowed_to, "B.Money",
                "local alias must win over the global one"
            );
            assert_eq!(
                global_to, "A.Money",
                "no local override -- global alias applies"
            );
        }
        _ => unreachable!(),
    }
}

// --- ambiguous marking (never guess) ---------------------------------

#[test]
fn ambiguous_via_using_step_stops_before_reaching_global_uniqueness() {
    let files = vec![
        (
            "A/Money.cs".to_string(),
            frag(vec![def("A.Money", "Money", "A", "class")], vec![], vec![]),
        ),
        (
            "B/Money.cs".to_string(),
            frag(vec![def("B.Money", "Money", "B", "class")], vec![], vec![]),
        ),
        (
            "C/Statement.cs".to_string(),
            frag(
                vec![def("C.Statement", "Statement", "C", "class")],
                vec![
                    FragUsing::Plain {
                        text: "A".into(),
                        global: false,
                    },
                    FragUsing::Plain {
                        text: "B".into(),
                        global: false,
                    },
                ],
                vec![type_ref("uses-type", "Money", None, "C")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(g.stats.ambiguous_count, 1);
    let edge = find_edge(&g, |e| matches!(e, Edge::Ambiguous { .. })).unwrap();
    match edge {
        Edge::Ambiguous {
            candidate_count,
            candidates,
            raw,
            ..
        } => {
            assert_eq!(*candidate_count, 2);
            assert_eq!(raw, "Money");
            assert_eq!(
                candidates.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
                vec!["A.Money", "B.Money"]
            );
        }
        _ => unreachable!(),
    }
}

#[test]
fn ambiguous_via_global_uniqueness_step_when_no_usings_apply() {
    let files = vec![
        (
            "A/Money.cs".to_string(),
            frag(vec![def("A.Money", "Money", "A", "class")], vec![], vec![]),
        ),
        (
            "B/Money.cs".to_string(),
            frag(vec![def("B.Money", "Money", "B", "class")], vec![], vec![]),
        ),
        (
            "C/Report.cs".to_string(),
            frag(
                vec![def("C.Report", "Report", "C", "class")],
                vec![],
                vec![type_ref("uses-type", "Money", None, "C")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(g.stats.ambiguous_count, 1);
    assert_eq!(g.stats.unresolved_external_count, 0);
}

#[test]
fn ambiguous_candidates_are_capped_at_five_sorted_by_id() {
    let mut files = Vec::new();
    for letter in ["E", "D", "C", "B", "A", "F", "G"] {
        files.push((
            format!("{letter}/Widget.cs"),
            frag(
                vec![def(&format!("{letter}.Widget"), "Widget", letter, "class")],
                vec![],
                vec![],
            ),
        ));
    }
    files.push((
        "Z/Probe.cs".to_string(),
        frag(
            vec![def("Z.Probe", "Probe", "Z", "class")],
            vec![],
            vec![type_ref("uses-type", "Widget", None, "Z")],
        ),
    ));
    let g = resolve_graph(&no_git_root(), &files);
    let edge = g
        .edges
        .iter()
        .find(|e| matches!(e, Edge::Ambiguous { .. }))
        .unwrap();
    match edge {
        Edge::Ambiguous {
            candidate_count,
            candidates,
            ..
        } => {
            assert_eq!(*candidate_count, 7);
            assert_eq!(candidates.len(), 5, "capped at AMBIGUOUS_CAP");
            let ids: Vec<&str> = candidates.iter().map(|c| c.id.as_str()).collect();
            let mut sorted = ids.clone();
            sorted.sort();
            assert_eq!(ids, sorted, "candidates must be sorted by id");
        }
        _ => unreachable!(),
    }
}

// --- enum-member asymmetry (the load-bearing case) --------------------

#[test]
fn enum_member_id_is_reachable_via_qualified_name_to_def() {
    let files = vec![(
        "A/Status.cs".to_string(),
        frag(
            vec![
                def("A.Status", "Status", "A", "enum"),
                def("A.Status.Active", "Active", "A", "enum-member"),
            ],
            vec![],
            vec![],
        ),
    )];
    let index = build_def_index(&files);
    assert!(index.qualified_name_to_def.contains_key("A.Status.Active"));
}

#[test]
fn enum_member_does_not_collide_with_a_same_named_class_via_global_uniqueness() {
    // Regression trap: if enum members were NOT excluded from
    // simple_name_to_defs, "Active" would have two candidates (the
    // class AND the enum member) and this reference would incorrectly
    // come back ambiguous instead of resolving to the class.
    let files = vec![
        (
            "A/Status.cs".to_string(),
            frag(
                vec![
                    def("A.Status", "Status", "A", "enum"),
                    def("A.Status.Active", "Active", "A", "enum-member"),
                ],
                vec![],
                vec![],
            ),
        ),
        (
            "B/Active.cs".to_string(),
            frag(
                vec![def("B.Active", "Active", "B", "class")],
                vec![],
                vec![],
            ),
        ),
        (
            "C/Toggle.cs".to_string(),
            frag(
                vec![def("C.Toggle", "Toggle", "C", "class")],
                vec![],
                vec![type_ref("uses-type", "Active", None, "C")],
            ),
        ),
    ];
    let g = resolve_graph(&no_git_root(), &files);
    let edge = find_edge(&g, |e| matches!(e, Edge::UsesType { .. }))
        .expect("must resolve cleanly, not go ambiguous");
    match edge {
        Edge::UsesType { to, .. } => assert_eq!(to, "B.Active"),
        _ => unreachable!(),
    }
    assert_eq!(g.stats.ambiguous_count, 0);
}
