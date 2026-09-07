use super::*;

// -----------------------------------------------------------------
// The TS/TSX reference fragment. The cross-file resolution
// these facts feed is tsgraph.rs's own test module; everything here is
// about what ONE file says, which is the whole of this extractor's job.
// -----------------------------------------------------------------

fn ts_fragment(src: &str, grammar: crate::parse::TsGrammar) -> TsFragment {
    let units = crate::parse::utf16_units(src);
    let tree = crate::parse::parse_ts_js(&units, grammar).expect("fixture parses");
    extract_ts_fragment(tree.root_node(), &crate::parse::utf16_bytes(&units))
}

fn ref_tuples(f: &TsFragment) -> Vec<(&str, &str, Option<&str>, usize)> {
    f.refs
        .iter()
        .map(|r| {
            (
                r.kind.as_str(),
                r.name.as_str(),
                r.member.as_deref(),
                r.line,
            )
        })
        .collect()
}

#[test]
fn an_import_clause_records_default_named_aliased_and_namespace_bindings() {
    let f = ts_fragment(
        "import Thing, { one, two as alias } from './m';\nimport * as ns from './n';\n",
        crate::parse::TsGrammar::Typescript,
    );
    assert_eq!(f.imports.len(), 2);
    assert_eq!(f.imports[0].spec, "./m");
    assert_eq!(f.imports[0].line, 1);
    let first: Vec<(&str, &str)> = f.imports[0]
        .bindings
        .iter()
        .map(|b| (b.local.as_str(), b.imported.as_str()))
        .collect();
    assert_eq!(
        first,
        vec![("Thing", "default"), ("one", "one"), ("alias", "two")]
    );
    let second: Vec<(&str, &str)> = f.imports[1]
        .bindings
        .iter()
        .map(|b| (b.local.as_str(), b.imported.as_str()))
        .collect();
    assert_eq!(
        second,
        vec![("ns", "*")],
        "a namespace clause binds '*', never a name of its own"
    );
}

#[test]
fn a_star_reexport_a_named_one_and_a_namespace_one_are_three_different_rows() {
    let f = ts_fragment(
        "export * from './a';\nexport { x as y } from './b';\nexport * as NS from './c';\n",
        crate::parse::TsGrammar::Typescript,
    );
    assert_eq!(f.reexports.len(), 3);
    assert!(f.reexports[0].star && f.reexports[0].names.is_empty());
    assert!(!f.reexports[1].star);
    assert_eq!(f.reexports[1].names[0].exported, "y");
    assert_eq!(f.reexports[1].names[0].imported, "x");
    assert!(
        !f.reexports[2].star && f.reexports[2].names.is_empty(),
        "`export * as NS` contributes its import edge and no name mapping"
    );
}

#[test]
fn only_names_this_file_declares_earn_a_def_and_the_default_name_is_recorded_separately() {
    let f = ts_fragment(
            "export function run() {}\nconst helper = 1;\nexport { helper };\nexport { missing } from './elsewhere';\nexport default run;\n",
            crate::parse::TsGrammar::Typescript,
        );
    let defs: Vec<(&str, &str, usize)> = f
        .defs
        .iter()
        .map(|d| (d.name.as_str(), d.kind.as_str(), d.line))
        .collect();
    assert_eq!(defs, vec![("run", "function", 1), ("helper", "const", 2)]);
    assert_eq!(
        f.default.as_deref(),
        Some("run"),
        "one def, two importable names -- never a second row"
    );
}

#[test]
fn a_commonjs_file_exports_by_name_and_binds_require_the_same_way_an_import_clause_does() {
    let f = ts_fragment(
            "const { logInfo } = require('./logger');\nconst bag = require('./logger');\nfunction report() { return logInfo(1) + bag.logInfo(2); }\nmodule.exports = { report };\n",
            crate::parse::TsGrammar::Javascript,
        );
    assert_eq!(
        f.defs.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
        vec!["report"]
    );
    let bindings: Vec<(&str, &str)> = f
        .imports
        .iter()
        .flat_map(|i| i.bindings.iter())
        .map(|b| (b.local.as_str(), b.imported.as_str()))
        .collect();
    assert_eq!(bindings, vec![("logInfo", "logInfo"), ("bag", "*")]);
    assert_eq!(
        ref_tuples(&f),
        vec![
            ("call", "logInfo", None, 3),
            ("call", "bag", Some("logInfo"), 3)
        ],
        "a member call records the QUALIFIER and its property, never the chain's tail alone"
    );
}

#[test]
fn a_dispatching_call_consumes_its_argument_so_it_is_never_also_a_plain_call() {
    let f = ts_fragment(
            "import { load, reset } from './actions';\nexport function go(dispatch) {\n  dispatch(load());\n  dispatch(reset);\n}\n",
            crate::parse::TsGrammar::Typescript,
        );
    assert_eq!(
        ref_tuples(&f),
        vec![
            ("dispatch", "load", None, 3),
            ("dispatch", "reset", None, 4)
        ],
        "the argument is the action creator; the outer dispatch(...) is not itself a call ref"
    );
}

#[test]
fn a_jsx_tag_is_a_reference_only_when_it_names_a_component_and_that_name_is_known() {
    let f = ts_fragment(
            "import { Card, Panel } from './ui';\nexport function View() {\n  return <div><Card /><Panel.Header /><section /><Unknown /></div>;\n}\n",
            crate::parse::TsGrammar::Tsx,
        );
    assert_eq!(
        ref_tuples(&f),
        vec![
            ("jsx-use", "Card", None, 3),
            ("jsx-use", "Panel", Some("Header"), 3)
        ],
        "lowercase tags are intrinsic elements; an unbound capitalised one is not a known name"
    );
}

#[test]
fn a_reference_to_a_name_this_file_neither_imports_nor_exports_is_never_recorded() {
    let f = ts_fragment(
        "export function go() {\n  const local = () => 1;\n  return local() + globalThing();\n}\n",
        crate::parse::TsGrammar::Typescript,
    );
    assert!(
        ref_tuples(&f).is_empty(),
        "the known-name filter is the contract, not an optimisation"
    );
}

#[test]
fn a_new_expression_records_the_constructor_as_a_call() {
    let f = ts_fragment(
        "import { Service } from './service';\nexport const make = () => new Service();\n",
        crate::parse::TsGrammar::Typescript,
    );
    assert_eq!(ref_tuples(&f), vec![("call", "Service", None, 2)]);
}

#[test]
fn a_file_with_no_default_export_keeps_the_shorter_fragment_shape() {
    let f = ts_fragment("export const x = 1;\n", crate::parse::TsGrammar::Typescript);
    assert_eq!(f.default, None);
    let json = serde_json::to_string(&f).unwrap();
    assert_eq!(
        json,
        r#"{"ts":1,"defs":[{"name":"x","kind":"const","line":1,"endLine":1}],"imports":[],"reexports":[],"refs":[]}"#
    );
}

#[test]
fn the_fragment_serializes_ts_first_and_default_last() {
    let f = ts_fragment(
        "export function run() {}\nexport default run;\n",
        crate::parse::TsGrammar::Typescript,
    );
    let json = serde_json::to_string(&f).unwrap();
    assert_eq!(
        json,
        r#"{"ts":1,"defs":[{"name":"run","kind":"function","line":1,"endLine":1}],"imports":[],"reexports":[],"refs":[],"default":"run"}"#
    );
}

#[test]
fn typescript_fragment_records_multiline_declaration_end_line() {
    let f = ts_fragment(
        "export interface Widget {\n  id: number;\n}\n",
        crate::parse::TsGrammar::Typescript,
    );
    assert_eq!((f.defs[0].line, f.defs[0].end_line), (1, 3));
}
