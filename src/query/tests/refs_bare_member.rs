use super::*;
use crate::query::refs::line_has_token;

// --- a bare member name, resolved by verifying edges at their line ---

fn name_row(name: &str, kind: &str, file: &str, line: usize, owner: &str) -> graph::GraphName {
    graph::GraphName {
        name: name.into(),
        kind: kind.into(),
        file: file.into(),
        line,
        owner: owner.into(),
    }
}

/// `Ledger` carries three inbound member edges (two of them one line
/// apart), and only one line names each of `Post`/`PostEx`. `Approve` is
/// declared on BOTH `Ledger` and `Journal`, each with one edge whose line
/// names it -- the ambiguity case, where verification survives on more than
/// one declaring type.
fn member_fixture() -> (graph::Graph, PathBuf) {
    let mut g = make_graph(
        vec![
            def(
                "App.Books.Ledger",
                "Ledger",
                "App.Books",
                "class",
                "Books/Ledger.cs",
                3,
            ),
            def(
                "App.Books.Journal",
                "Journal",
                "App.Books",
                "class",
                "Books/Journal.cs",
                3,
            ),
            def(
                "App.Books.Consumer",
                "Consumer",
                "App.Books",
                "class",
                "Books/Consumer.cs",
                1,
            ),
        ],
        vec![
            uses_member(
                "Books/Consumer.cs",
                1,
                "App.Books.Ledger",
                "Books/Ledger.cs",
            ),
            uses_member(
                "Books/Consumer.cs",
                2,
                "App.Books.Ledger",
                "Books/Ledger.cs",
            ),
            uses_member(
                "Books/Consumer.cs",
                3,
                "App.Books.Ledger",
                "Books/Ledger.cs",
            ),
            uses_member(
                "Books/Consumer.cs",
                4,
                "App.Books.Journal",
                "Books/Journal.cs",
            ),
        ],
    );
    g.names = vec![
        name_row("Ledger", "class", "Books/Ledger.cs", 3, ""),
        name_row("Post", "method", "Books/Ledger.cs", 5, "App.Books.Ledger"),
        name_row("PostEx", "method", "Books/Ledger.cs", 7, "App.Books.Ledger"),
        name_row(
            "Reconcile",
            "method",
            "Books/Ledger.cs",
            9,
            "App.Books.Ledger",
        ),
        name_row(
            "Approve",
            "method",
            "Books/Ledger.cs",
            11,
            "App.Books.Ledger",
        ),
        name_row("Journal", "class", "Books/Journal.cs", 3, ""),
        name_row(
            "Approve",
            "method",
            "Books/Journal.cs",
            5,
            "App.Books.Journal",
        ),
    ];
    let root = temp_repo_root("bare-member");
    write_manifest_fixture(
        &root,
        &["Books/Ledger.cs", "Books/Journal.cs", "Books/Consumer.cs"],
    );
    fs::create_dir_all(root.join("Books")).expect("fixture dir");
    fs::write(
        root.join("Books/Consumer.cs"),
        "Ledger.Post(1);\nLedger.PostEx(2);\nLedger.Approve(3);\nJournal.Approve(4);\n",
    )
    .expect("fixture file");
    (g, root)
}

fn member_models(index: &GraphIndex, query: &str, inbound_cap: usize) -> Vec<RefsModel> {
    match build_refs_model(
        index,
        query,
        false,
        DEFAULT_CAP,
        inbound_cap,
        OUTBOUND_CAP,
        false,
    ) {
        RefsResult::Members(models) => models,
        other => panic!("expected Members, got {other:?}"),
    }
}

#[test]
fn line_has_token_matches_whole_tokens_only() {
    assert!(line_has_token("Ledger.Post(1);", "Post"));
    assert!(line_has_token("Post", "Post"));
    assert!(!line_has_token("Ledger.PostEx(2);", "Post"));
    assert!(!line_has_token("RePost(2);", "Post"));
    assert!(!line_has_token("Post_x();", "Post"));
    assert!(!line_has_token("x1Post();", "Post"));
    // Every code point outside ASCII is a boundary: this reads the
    // neighbouring UTF-8 byte, which is never an ASCII word character.
    assert!(line_has_token("\u{2026}Post\u{2026}", "Post"));
    assert!(!line_has_token("", "Post"));
    assert!(!line_has_token("Post", ""));
}

#[test]
fn build_refs_model_bare_member_keeps_only_the_edges_whose_line_names_it() {
    let (g, root) = member_fixture();
    let index = load_graph_index(&g, &root);
    let models = member_models(&index, "PostEx", INBOUND_CAP);

    assert_eq!(models.len(), 1);
    let m = &models[0];
    assert_eq!(m.id, "App.Books.Ledger.PostEx");
    assert_eq!(m.kind, "member");
    assert_eq!(
        m.sites,
        vec![DefSite {
            file: "Books/Ledger.cs".into(),
            line: 7
        }]
    );
    assert_eq!(
        m.inbound.uses_member.total, 1,
        "the type has three inbound member edges; one names PostEx"
    );
    assert_eq!(
        m.inbound.uses_member.rows,
        vec![InboundRow {
            file: "Books/Consumer.cs".into(),
            line: 2,
            heuristic: false,
            tier: None,
            source: "Ledger.PostEx(2);".into()
        }]
    );
    assert_eq!(m.inbound.inherits.total, 0);
    assert!(
        m.outbound.is_none(),
        "a member answer never carries the outbound tables"
    );
}

#[test]
fn build_refs_model_bare_member_refuses_a_longer_identifier_that_starts_with_the_query() {
    let (g, root) = member_fixture();
    let index = load_graph_index(&g, &root);
    let models = member_models(&index, "Post", INBOUND_CAP);
    let ledger = models
        .iter()
        .find(|m| m.id == "App.Books.Ledger.Post")
        .expect("Ledger declares Post");

    assert_eq!(
        ledger
            .inbound
            .uses_member
            .rows
            .iter()
            .map(|r| r.line)
            .collect::<Vec<_>>(),
        vec![1],
        "line 2 is `Ledger.PostEx(2);` -- a substring hit, not a token hit"
    );
}

// `Approve` is declared on Ledger AND Journal, and both survive edge-line
// verification (unlike the fixture's own `Post`, now single-owner): the
// answer is the ambiguous candidate list, in name-index order, never a
// `Members` block per type -- the same house rule of never guessing between
// candidates that an ambiguous TYPE name already answers with.
#[test]
fn build_refs_model_bare_member_verified_on_several_types_answers_ambiguous_not_members() {
    let (g, root) = member_fixture();
    let index = load_graph_index(&g, &root);
    let model = build_refs_model(
        &index,
        "Approve",
        false,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    );

    assert_eq!(
        model,
        RefsResult::Ambiguous(vec![
            "App.Books.Ledger".to_string(),
            "App.Books.Journal".to_string()
        ])
    );
}

#[test]
fn build_refs_model_bare_member_ambiguous_across_types_answers_the_same_regardless_of_inbound_cap()
{
    let (g, root) = member_fixture();
    let index = load_graph_index(&g, &root);
    let model = build_refs_model(
        &index,
        "Approve",
        false,
        DEFAULT_CAP,
        1,
        OUTBOUND_CAP,
        false,
    );

    assert_eq!(
        model,
        RefsResult::Ambiguous(vec![
            "App.Books.Ledger".to_string(),
            "App.Books.Journal".to_string()
        ])
    );
}

#[test]
fn build_refs_model_bare_member_with_no_verified_edge_stays_not_found() {
    let (g, root) = member_fixture();
    let index = load_graph_index(&g, &root);

    assert_eq!(
        build_refs_model(
            &index,
            "Reconcile",
            false,
            DEFAULT_CAP,
            INBOUND_CAP,
            OUTBOUND_CAP,
            false
        ),
        RefsResult::NotFound
    );
    assert_eq!(
        build_refs_model(
            &index,
            "NoSuchMemberAnywhere",
            false,
            DEFAULT_CAP,
            INBOUND_CAP,
            OUTBOUND_CAP,
            false
        ),
        RefsResult::NotFound
    );
}

#[test]
fn build_refs_model_prefers_a_type_over_a_member_of_the_same_name() {
    let (g, root) = member_fixture();
    let index = load_graph_index(&g, &root);

    let RefsResult::Resolved(model) = build_refs_model(
        &index,
        "Ledger",
        false,
        DEFAULT_CAP,
        INBOUND_CAP,
        OUTBOUND_CAP,
        false,
    ) else {
        panic!("Ledger is a type and must resolve as one");
    };
    assert_eq!(model.id, "App.Books.Ledger");
}
