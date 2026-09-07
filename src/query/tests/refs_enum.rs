use super::*;

#[test]
fn build_refs_model_on_an_enum_appends_member_refs_in_declaration_order() {
    let graph = make_graph(
        vec![
            def(
                "App.Flags.Toggles",
                "Toggles",
                "App.Flags",
                "enum",
                "Flags/Toggles.cs",
                3,
            ),
            def(
                "App.Flags.Toggles.EnableX",
                "EnableX",
                "App.Flags",
                "enum-member",
                "Flags/Toggles.cs",
                5,
            ),
            def(
                "App.Flags.Toggles.EnableY",
                "EnableY",
                "App.Flags",
                "enum-member",
                "Flags/Toggles.cs",
                6,
            ),
            def(
                "App.Flags.Toggles.EnableZ",
                "EnableZ",
                "App.Flags",
                "enum-member",
                "Flags/Toggles.cs",
                7,
            ),
            def(
                "App.Run.Runner",
                "Runner",
                "App.Run",
                "class",
                "Run/Runner.cs",
                3,
            ),
        ],
        vec![
            uses_type("Run/Runner.cs", 5, "App.Flags.Toggles", "Flags/Toggles.cs"),
            uses_member(
                "Run/Runner.cs",
                6,
                "App.Flags.Toggles.EnableY",
                "Flags/Toggles.cs",
            ),
            uses_member(
                "Run/Runner.cs",
                7,
                "App.Flags.Toggles.EnableX",
                "Flags/Toggles.cs",
            ),
            uses_member(
                "Run/Runner.cs",
                8,
                "App.Flags.Toggles.EnableX",
                "Flags/Toggles.cs",
            ),
        ],
    );
    let root = temp_repo_root("enum-member-refs");
    write_manifest_fixture(&root, &["Flags/Toggles.cs", "Run/Runner.cs"]);
    let index = load_graph_index(&graph, &root);

    let model = match build_refs_model(
        &index,
        "Toggles",
        false,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        model.member_refs,
        Some(MemberRefs {
            total: 3,
            member_count: 2,
            members: vec![
                MemberRefEntry { name: "EnableX".into(), count: 2 },
                MemberRefEntry { name: "EnableY".into(), count: 1 },
            ],
            dropped: 0,
        }),
        "declaration order, never count order, and a member nothing references is left out entirely"
    );
    assert_eq!(
        model.inbound.uses_member.total, 3,
        "the existing union is unchanged by the split"
    );

    let other = match build_refs_model(
        &index,
        "Runner",
        false,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(
        other.member_refs, None,
        "nothing but an enum carries the field"
    );
}

// --- 15: build_refs_model on an enum member ---

#[test]
fn build_refs_model_on_enum_member_def_site_plus_inbound_uses_member() {
    let graph = enum_fixture_graph();
    let root = enum_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_refs_model(
        &index,
        "Question",
        true,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(model.kind, "enum-member");
    assert_eq!(
        model.sites,
        vec![DefSite {
            file: "Enums/PostType.cs".into(),
            line: 6
        }]
    );
    assert_eq!(model.inbound.uses_member.total, 1);
    assert_eq!(
        model.inbound.uses_member.rows[0].file,
        "Consumers/Reader.cs"
    );
    assert_eq!(model.inbound.uses_member.rows[0].line, 8);
}

// --- 16: build_refs_model on the enum itself: members' inbound unions in ---

#[test]
fn build_refs_model_on_enum_itself_unions_members_inbound_uses_member() {
    let graph = enum_fixture_graph();
    let root = enum_fixture_root();
    let index = load_graph_index(&graph, &root);
    let model = match build_refs_model(
        &index,
        "PostType",
        true,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Resolved(m) => m,
        other => panic!("expected Resolved, got {other:?}"),
    };
    assert_eq!(model.kind, "enum");
    assert_eq!(
        model.inbound.uses_member.total, 1,
        "member-level access must surface on the enum query"
    );
    assert_eq!(
        model.inbound.uses_member.rows[0].file,
        "Consumers/Reader.cs"
    );
    assert_eq!(model.inbound.uses_member.rows[0].line, 8);
}

// --- 17: two enums, each with a same-named member -> ambiguous ---

#[test]
fn two_enums_with_same_named_member_resolve_ambiguous_both_sites_surfaced() {
    let graph = make_graph(
        vec![
            def(
                "App.One.StatusEnum",
                "StatusEnum",
                "App.One",
                "enum",
                "One/StatusEnum.cs",
                1,
            ),
            def(
                "App.One.StatusEnum.Changed",
                "Changed",
                "App.One",
                "enum-member",
                "One/StatusEnum.cs",
                2,
            ),
            def(
                "App.Two.OtherEnum",
                "OtherEnum",
                "App.Two",
                "enum",
                "Two/OtherEnum.cs",
                1,
            ),
            def(
                "App.Two.OtherEnum.Changed",
                "Changed",
                "App.Two",
                "enum-member",
                "Two/OtherEnum.cs",
                2,
            ),
        ],
        vec![],
    );
    let root = temp_repo_root("two-enums-changed");
    write_manifest_fixture(&root, &["One/StatusEnum.cs", "Two/OtherEnum.cs"]);
    let index = load_graph_index(&graph, &root);

    match resolve_symbol(&index, "Changed") {
        Resolution::Ambiguous(mut ids) => {
            ids.sort();
            assert_eq!(
                ids,
                vec![
                    "App.One.StatusEnum.Changed".to_string(),
                    "App.Two.OtherEnum.Changed".to_string()
                ]
            );
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }

    match build_refs_model(
        &index,
        "Changed",
        true,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Ambiguous(mut ids) => {
            ids.sort();
            assert_eq!(
                ids,
                vec![
                    "App.One.StatusEnum.Changed".to_string(),
                    "App.Two.OtherEnum.Changed".to_string()
                ]
            );
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}
