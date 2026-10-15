use crate::extract::{
    TsBinding, TsDynamicImport, TsFragment, TsFragmentDef, TsImport, TsReexport, TsReexportName,
    TsRef,
};
use crate::graph::Edge;
use crate::tsgraph::{resolve_ts_graph, TsAliasScopes};

fn file(defs: &[&str], imports: Vec<TsImport>, reexports: Vec<TsReexport>) -> TsFragment {
    TsFragment {
        ts: 1,
        defs: defs
            .iter()
            .enumerate()
            .map(|(i, name)| TsFragmentDef {
                name: (*name).into(),
                kind: "function".into(),
                line: i + 1,
                end_line: i + 1,
            })
            .collect(),
        imports,
        reexports,
        refs: Vec::new(),
        default: None,
        ..TsFragment::default()
    }
}

fn import(spec: &str, line: usize, bindings: &[(&str, &str)]) -> TsImport {
    TsImport {
        spec: spec.into(),
        line,
        bindings: bindings
            .iter()
            .map(|(local, imported)| TsBinding {
                local: (*local).into(),
                imported: (*imported).into(),
            })
            .collect(),
        require: false,
    }
}

fn reexport(spec: &str, line: usize, names: &[&str]) -> TsReexport {
    TsReexport {
        spec: spec.into(),
        line,
        star: names.is_empty(),
        names: names
            .iter()
            .map(|n| TsReexportName {
                exported: (*n).into(),
                imported: (*n).into(),
            })
            .collect(),
        namespace: None,
    }
}

/// `(from_file, from_line, to_file, via, bound_defs)` of every import edge.
type ImportSite = (String, usize, String, Option<String>, Vec<String>);

fn imports(fragments: &[(String, TsFragment)]) -> Vec<ImportSite> {
    resolve_ts_graph(fragments, &TsAliasScopes::default())
        .edges
        .into_iter()
        .filter_map(|e| match e {
            Edge::Import {
                from_file,
                from_line,
                to_file,
                via,
                bound_defs,
                ..
            } => Some((from_file, from_line, to_file, via, bound_defs)),
            _ => None,
        })
        .collect()
}

fn site(from: &str, line: usize, to: &str, via: Option<&str>, bound: &[&str]) -> ImportSite {
    (
        from.into(),
        line,
        to.into(),
        via.map(Into::into),
        bound.iter().map(|b| (*b).to_string()).collect(),
    )
}

// A declaring module, a barrel re-exporting it wholesale, and a second
// declaring module the barrel re-exports one name from.
fn library() -> Vec<(String, TsFragment)> {
    vec![
        ("lib/a.ts".into(), file(&["alpha", "beta"], vec![], vec![])),
        ("lib/b.ts".into(), file(&["gamma"], vec![], vec![])),
        (
            "lib/index.ts".into(),
            file(
                &[],
                vec![],
                vec![reexport("./a", 1, &[]), reexport("./b", 2, &["gamma"])],
            ),
        ),
    ]
}

fn with(consumer: TsFragment) -> Vec<(String, TsFragment)> {
    let mut frags = library();
    frags.push(("app/use.ts".into(), consumer));
    frags
}

fn consumer_imports(frags: &[(String, TsFragment)]) -> Vec<ImportSite> {
    imports(frags)
        .into_iter()
        .filter(|s| s.0 == "app/use.ts")
        .collect()
}

#[test]
fn module_resolution_ac_5_export_equals_binds_import_equals_and_default() {
    let mut source = file(&["Thing"], vec![], vec![]);
    source.export_equals = Some("Thing".into());
    let mut require = import("../lib/a", 1, &[("local", "*")]);
    require.require = true;
    let mut consumer = file(
        &[],
        vec![require, import("../lib/a", 2, &[("other", "default")])],
        vec![],
    );
    consumer.refs.push(TsRef {
        kind: "call".into(),
        name: "local".into(),
        member: None,
        line: 3,
    });
    let fragments = vec![("lib/a.ts".into(), source), ("app/use.ts".into(), consumer)];
    assert_eq!(
        consumer_imports(&fragments),
        vec![
            site("app/use.ts", 1, "lib/a.ts", None, &["lib/a.ts#Thing"]),
            site("app/use.ts", 2, "lib/a.ts", None, &["lib/a.ts#Thing"]),
        ]
    );
    assert!(resolve_ts_graph(&fragments, &TsAliasScopes::default())
        .edges
        .iter()
        .any(|edge| { matches!(edge, Edge::Call { to, .. } if to == "lib/a.ts#Thing") }));
}

#[test]
fn module_resolution_ac_5_namespace_reexport_reaches_member() {
    let mut namespace = reexport("./a", 1, &[]);
    namespace.star = false;
    namespace.namespace = Some("ns".into());
    let mut consumer = file(
        &[],
        vec![import("../lib/index", 1, &[("ns", "ns")])],
        vec![],
    );
    consumer.refs.push(TsRef {
        kind: "call".into(),
        name: "ns".into(),
        member: Some("alpha".into()),
        line: 2,
    });
    let fragments = vec![
        ("lib/a.ts".into(), file(&["alpha"], vec![], vec![])),
        ("lib/index.ts".into(), file(&[], vec![], vec![namespace])),
        ("app/use.ts".into(), consumer),
    ];
    assert!(resolve_ts_graph(&fragments, &TsAliasScopes::default())
        .edges
        .iter()
        .any(|edge| { matches!(edge, Edge::Call { to, .. } if to == "lib/a.ts#alpha") }));
}

#[test]
fn module_resolution_ac_5_literal_dynamic_import_emits_an_import_edge() {
    let mut consumer = file(&[], vec![], vec![]);
    consumer.dynamic_imports.push(TsDynamicImport {
        spec: "../lib/a".into(),
        line: 2,
    });
    let fragments = vec![
        ("lib/a.ts".into(), file(&[], vec![], vec![])),
        ("app/use.ts".into(), consumer),
    ];
    assert_eq!(
        consumer_imports(&fragments),
        vec![site("app/use.ts", 2, "lib/a.ts", None, &[])]
    );
}

#[test]
fn a_named_import_binds_on_the_statement_edge() {
    let frags = with(file(
        &[],
        vec![import("../lib/a", 1, &[("alpha", "alpha")])],
        vec![],
    ));
    assert_eq!(
        consumer_imports(&frags),
        vec![site("app/use.ts", 1, "lib/a.ts", None, &["lib/a.ts#alpha"])]
    );
}

#[test]
fn a_default_import_binds_the_default_export() {
    let mut frags = with(file(
        &[],
        vec![import("../lib/a", 1, &[("x", "default")])],
        vec![],
    ));
    frags[0].1.default = Some("beta".into());
    assert_eq!(
        consumer_imports(&frags),
        vec![site("app/use.ts", 1, "lib/a.ts", None, &["lib/a.ts#beta"])]
    );
}

#[test]
fn an_aliased_import_binds_the_original_definition() {
    let frags = with(file(
        &[],
        vec![import("../lib/a", 1, &[("renamed", "beta")])],
        vec![],
    ));
    assert_eq!(
        consumer_imports(&frags)[0].4,
        vec!["lib/a.ts#beta".to_string()]
    );
}

#[test]
fn two_names_from_one_file_bind_in_binding_order_once_each() {
    let frags = with(file(
        &[],
        vec![import(
            "../lib/a",
            1,
            &[("beta", "beta"), ("alpha", "alpha"), ("again", "beta")],
        )],
        vec![],
    ));
    assert_eq!(
        consumer_imports(&frags)[0].4,
        vec!["lib/a.ts#beta".to_string(), "lib/a.ts#alpha".to_string()]
    );
}

#[test]
fn a_barrel_import_binds_on_the_via_edge_of_each_declaring_file() {
    let frags = with(file(
        &[],
        vec![import(
            "../lib",
            3,
            &[("alpha", "alpha"), ("gamma", "gamma"), ("beta", "beta")],
        )],
        vec![],
    ));
    assert_eq!(
        consumer_imports(&frags),
        vec![
            site("app/use.ts", 3, "lib/index.ts", None, &[]),
            site(
                "app/use.ts",
                3,
                "lib/a.ts",
                Some("lib/index.ts"),
                &["lib/a.ts#alpha", "lib/a.ts#beta"]
            ),
            site(
                "app/use.ts",
                3,
                "lib/b.ts",
                Some("lib/index.ts"),
                &["lib/b.ts#gamma"]
            ),
        ]
    );
}

#[test]
fn namespace_side_effect_and_unknown_names_bind_nothing() {
    let frags = with(file(
        &[],
        vec![
            import("../lib/a", 1, &[("ns", "*")]),
            import("../lib/b", 2, &[]),
            import("../lib/a", 3, &[("missing", "missing")]),
        ],
        vec![],
    ));
    assert_eq!(
        consumer_imports(&frags),
        vec![
            site("app/use.ts", 1, "lib/a.ts", None, &[]),
            site("app/use.ts", 2, "lib/b.ts", None, &[]),
            site("app/use.ts", 3, "lib/a.ts", None, &[]),
        ]
    );
}

#[test]
fn a_named_re_export_binds_only_a_name_its_target_declares() {
    let frags = with(file(
        &[],
        vec![],
        vec![
            reexport("../lib/a", 1, &["alpha"]),
            reexport("../lib", 2, &["alpha"]),
            reexport("../lib/b", 3, &[]),
        ],
    ));
    assert_eq!(
        consumer_imports(&frags),
        vec![
            site("app/use.ts", 1, "lib/a.ts", None, &["lib/a.ts#alpha"]),
            site("app/use.ts", 2, "lib/index.ts", None, &[]),
            site("app/use.ts", 3, "lib/b.ts", None, &[]),
        ]
    );
}

// Without `bound_defs` the edges are exactly the ones the resolver wrote
// before the field existed: same count, order, lines, targets and `via`.
#[test]
fn binding_adds_no_edge_and_changes_no_other_field() {
    let frags = with(file(
        &[],
        vec![
            import("../lib", 1, &[("alpha", "alpha"), ("gamma", "gamma")]),
            import("../lib/a", 2, &[("ns", "*")]),
            import("../lib/b", 3, &[("gamma", "gamma")]),
        ],
        vec![reexport("../lib/a", 4, &["beta"])],
    ));
    let stripped: Vec<(String, usize, String, Option<String>)> = imports(&frags)
        .into_iter()
        .map(|(from, line, to, via, _)| (from, line, to, via))
        .collect();
    let expected = [
        ("lib/index.ts", 1, "lib/a.ts", None),
        ("lib/index.ts", 2, "lib/b.ts", None),
        ("app/use.ts", 1, "lib/index.ts", None),
        ("app/use.ts", 1, "lib/a.ts", Some("lib/index.ts")),
        ("app/use.ts", 1, "lib/b.ts", Some("lib/index.ts")),
        ("app/use.ts", 2, "lib/a.ts", None),
        ("app/use.ts", 3, "lib/b.ts", None),
        ("app/use.ts", 4, "lib/a.ts", None),
    ];
    let expected: Vec<(String, usize, String, Option<String>)> = expected
        .iter()
        .map(|(f, l, t, v)| ((*f).into(), *l, (*t).into(), v.map(Into::into)))
        .collect();
    assert_eq!(stripped, expected);
}

#[test]
fn every_bound_definition_is_declared_in_its_edges_to_file() {
    let frags = with(file(
        &[],
        vec![
            import(
                "../lib",
                1,
                &[("alpha", "alpha"), ("gamma", "gamma"), ("beta", "beta")],
            ),
            import("../lib/a", 2, &[("alpha", "alpha"), ("alpha2", "alpha")]),
        ],
        vec![
            reexport("../lib", 3, &["gamma"]),
            reexport("../lib/b", 4, &["gamma"]),
        ],
    ));
    let mut bound = 0;
    for (_, _, to_file, _, defs) in imports(&frags) {
        let mut seen = std::collections::HashSet::new();
        for d in &defs {
            assert_eq!(
                d.split('#').next(),
                Some(to_file.as_str()),
                "{d} on {to_file}"
            );
            assert!(seen.insert(d.clone()), "{d} listed twice");
            bound += 1;
        }
    }
    assert_eq!(bound, 6);
}
