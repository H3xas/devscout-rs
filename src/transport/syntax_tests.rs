use super::*;

fn parse(source: &str) -> (tree_sitter::Tree, Vec<u8>) {
    let units = crate::parse::utf16_units(source);
    let tree = parser().parse_utf16_le(&units, None).expect("parse");
    (tree, crate::parse::utf16_bytes(&units))
}

fn joined_value(root: Node) -> Option<Node> {
    if root.kind() == "binary_expression" {
        return Some(root);
    }
    named(root).into_iter().find_map(joined_value)
}

#[test]
fn transport_facts_ac_6_a_literal_is_found_again_at_its_position() {
    let source =
        "class C { const string Ünit = \"é\"; string A = \"rabbitmq://u:p\" + Parts.Tail; }";
    let (tree, src) = parse(source);
    let joined = joined_value(tree.root_node()).expect("joined value");
    let all = pieces(joined, &src).expect("pieces");
    let [Piece::Lit(text, at), Piece::Ref(path)] = all.as_slice() else {
        panic!("unexpected pieces {all:?}");
    };
    assert_eq!(
        (text.as_str(), path.as_str()),
        ("rabbitmq://u:p", "Parts.Tail")
    );
    let found = literal_at(tree.root_node(), *at).expect("literal at its position");
    assert_eq!(string_literal(found, &src), Some(Ok(text.clone())));

    let moved = Position {
        line: at.line,
        column: at.column + 2,
    };
    assert!(literal_at(tree.root_node(), moved).is_none());
    let past = Position {
        line: at.line + 1,
        column: 0,
    };
    assert!(literal_at(tree.root_node(), past).is_none());
}

#[test]
fn transport_facts_ac_6_a_cached_joined_value_holds_no_literal_text() {
    let source = "using System;\nusing MassTransit;\nclass C {\n    ISendEndpointProvider p;\n    async void M() {\n        await p.GetSendEndpoint(new Uri(\"rabbitmq://u:s3cret-unit\" + Parts.Tail));\n        await p.GetSendEndpoint(new Uri(Parts.Head + \"s3cret-middle\" + Parts.Tail));\n    }\n}\n";
    let (tree, src) = parse(source);
    let sites = crate::transport::sites(source, tree.root_node(), &src);
    let cached = serde_json::to_string(&sites).expect("serialize");
    assert!(cached.contains("Parts.Tail"), "{cached}");
    assert!(!cached.contains("s3cret"), "{cached}");
}
