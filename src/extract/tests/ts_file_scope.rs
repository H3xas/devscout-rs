use super::*;

fn fragment(source: &str, grammar: crate::parse::TsGrammar) -> TsFragment {
    let units = crate::parse::utf16_units(source);
    let tree = crate::parse::parse_ts_js(&units, grammar).unwrap();
    extract_ts_fragment(tree.root_node(), &crate::parse::utf16_bytes(&units))
}

#[test]
fn file_scope_defs_ac_1_fragment_marks_only_non_exported_declarations() {
    let source = "export function publicName() {}\nfunction localName() {}\nconst bound = require('m'), kept = 1;\n";
    for grammar in [
        crate::parse::TsGrammar::Javascript,
        crate::parse::TsGrammar::Typescript,
    ] {
        let f = fragment(source, grammar);
        assert_eq!(
            f.defs
                .iter()
                .map(|d| (d.name.as_str(), d.local))
                .collect::<Vec<_>>(),
            vec![("publicName", false), ("localName", true), ("kept", true)]
        );
        let json = serde_json::to_value(&f.defs).unwrap();
        assert!(json[0].get("local").is_none());
        assert_eq!(json[1]["local"], true);
        assert_eq!(json[2]["local"], true);
    }
}

#[test]
fn file_scope_defs_ac_2_nested_wrappers_are_collected_in_source_order() {
    let source = "function first() {}\n(function() { function inner() {} (() => { function deep() {} })(); })();\n";
    for grammar in [
        crate::parse::TsGrammar::Javascript,
        crate::parse::TsGrammar::Typescript,
    ] {
        let f = fragment(source, grammar);
        assert_eq!(
            f.defs.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            vec!["first", "inner", "deep"]
        );
    }
}
