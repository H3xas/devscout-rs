use std::fs;
use std::path::Path;

use crate::extract;

use super::fragment_types::{
    FragDef, FragExtensionMethod, FragFact, FragLambdaSlot, FragName, FragRef, FragUsing, Fragment,
};
use super::ordered::OrderedMap;

/// Build a graph fragment from this crate's extractor output.
/// `Extraction.purpose` has no fragment
/// counterpart -- the fragment is graph-only.
#[allow(
    clippy::too_many_lines,
    reason = "one ordered field-by-field translation from Extraction into Fragment; splitting it would separate fields that must stay mapped consistently"
)]
pub fn fragment_from_extraction(e: &extract::Extraction) -> Fragment {
    Fragment {
        defs: e
            .defs
            .iter()
            .map(|d| FragDef {
                id: d.id.clone(),
                name: d.name.clone(),
                namespace: d.namespace.clone(),
                kind: d.kind.clone(),
                line: d.line,
                methods: d.methods.clone(),
                properties: d.properties.clone(),
                fields: d.fields.clone(),
                method_returns: {
                    let mut m = OrderedMap::new();
                    for (name, returns) in &d.method_returns {
                        m.insert(name.clone(), returns.clone());
                    }
                    m
                },
                extension_methods: d
                    .extension_methods
                    .iter()
                    .map(|e| FragExtensionMethod {
                        name: e.name.clone(),
                        this_type: e.this_type.clone(),
                        arity_min: e.arity_min,
                        arity_max: e.arity_max,
                        this_args: e.this_args.clone(),
                    })
                    .collect(),
                bases: d.bases.clone(),
                type_params: d.type_params.clone(),
                base_generic_args: {
                    let mut m = OrderedMap::new();
                    for (name, args) in &d.base_generic_args {
                        m.insert(name.clone(), args.clone());
                    }
                    m
                },
                test_methods: d.test_methods.clone(),
                property_types: {
                    let mut m = OrderedMap::new();
                    for (name, fact) in &d.property_types {
                        m.insert(
                            name.clone(),
                            FragFact {
                                type_name: fact.type_name.clone(),
                                args: fact.args.clone(),
                            },
                        );
                    }
                    m
                },
                field_types: {
                    let mut m = OrderedMap::new();
                    for (name, fact) in &d.field_types {
                        m.insert(
                            name.clone(),
                            FragFact {
                                type_name: fact.type_name.clone(),
                                args: fact.args.clone(),
                            },
                        );
                    }
                    m
                },
                method_return_args: {
                    let mut m = OrderedMap::new();
                    for (name, args) in &d.method_return_args {
                        m.insert(name.clone(), args.clone());
                    }
                    m
                },
                non_public_methods: d.non_public_methods.clone(),
                method_arities: {
                    let mut m = OrderedMap::new();
                    for (name, ranges) in &d.method_arities {
                        m.insert(name.clone(), ranges.clone());
                    }
                    m
                },
                method_params: {
                    let mut m = OrderedMap::new();
                    for (name, overloads) in &d.method_params {
                        m.insert(name.clone(), overloads.clone());
                    }
                    m
                },
                end_line: d.end_line,
            })
            .collect(),
        usings: e
            .usings
            .iter()
            .map(|u| match u {
                extract::UsingRecord::Alias {
                    alias,
                    target,
                    global,
                } => FragUsing::Alias {
                    alias: alias.clone(),
                    target: target.clone(),
                    global: *global,
                },
                extract::UsingRecord::Plain { text, global } => FragUsing::Plain {
                    text: text.clone(),
                    global: *global,
                },
            })
            .collect(),
        refs: e
            .refs
            .iter()
            .map(|r| FragRef {
                kind: r.kind.clone(),
                name: r.name.clone(),
                qualified: r.qualified.clone(),
                member: r.member.clone(),
                line: r.line,
                namespace: r.namespace.clone(),
                type_arg_count: r.type_arg_count,
                generic: r.generic,
                receiver_type: r.receiver_type.clone(),
                arg_count: r.arg_count,
                receiver_args: r.receiver_args.clone(),
                outer_types: r.outer_types.clone(),
                args: r.args.clone(),
                receiver_property_owner: r.receiver_property_owner.clone(),
                receiver_call_owner: r.receiver_call_owner.clone(),
                receiver_call_member: r.receiver_call_member.clone(),
                receiver_base: r.receiver_base,
                receiver_awaited: r.receiver_awaited,
                receiver_local: r.receiver_local,
                receiver_lambda: r.receiver_lambda.as_ref().map(|s| FragLambdaSlot {
                    owner: s.owner.clone(),
                    member: s.member.clone(),
                    arg_count: s.arg_count,
                    arg_index: s.arg_index,
                    arity: s.arity,
                    index: s.index,
                }),
            })
            .collect(),
        names: e
            .names
            .iter()
            .map(|n| FragName {
                name: n.name.clone(),
                kind: n.kind.clone(),
                line: n.line,
                owner: n.owner.clone(),
            })
            .collect(),
    }
}

/// Markup and resource files never reach the extractor: a
/// line scan reads them instead (see markup.rs). They still get a fragment even
/// when they declare nothing, because the fragments index is what tells `map`
/// whether the graph's input set changed, and a file missing from it reads as a
/// permanent mismatch. `usings` is always empty: XAML has no using-directive
/// equivalent, and the `xmlns:` prefix declarations that stand in for one are
/// already resolved into fully-qualified ref names by the scan.
pub fn markup_fragment(root: &Path, rel: &str) -> Option<Fragment> {
    // Lossy read: an invalid byte becomes U+FFFD rather than dropping the
    // file.
    let text = String::from_utf8_lossy(&fs::read(root.join(rel)).ok()?).into_owned();
    let facts = crate::markup::markup_facts(rel, &text);
    Some(Fragment {
        defs: facts
            .defs
            .into_iter()
            .map(|d| FragDef {
                id: d.id,
                name: d.name,
                namespace: d.namespace,
                kind: d.kind,
                line: d.line,
                methods: Vec::new(),
                properties: Vec::new(),
                fields: Vec::new(),
                method_returns: OrderedMap::new(),
                extension_methods: Vec::new(),
                bases: Vec::new(),
                type_params: Vec::new(),
                base_generic_args: OrderedMap::new(),
                test_methods: Vec::new(),
                property_types: OrderedMap::new(),
                field_types: OrderedMap::new(),
                method_return_args: OrderedMap::new(),
                non_public_methods: Vec::new(),
                method_arities: OrderedMap::new(),
                method_params: OrderedMap::new(),
                end_line: d.line,
            })
            .collect(),
        usings: Vec::new(),
        refs: facts
            .refs
            .into_iter()
            .map(|r| FragRef {
                kind: r.kind,
                name: r.name,
                qualified: r.qualified,
                member: r.member,
                line: r.line,
                // A markup ref site has no enclosing namespace of its own, and
                // the empty string is what a C# ref at file scope carries too --
                // never `None`, which is the 'imports' spelling.
                namespace: Some(String::new()),
                type_arg_count: Some(0),
                generic: false,
                receiver_type: None,
                arg_count: None,
                receiver_args: None,
                outer_types: Vec::new(),
                args: None,
                receiver_property_owner: None,
                receiver_call_owner: None,
                receiver_call_member: None,
                receiver_base: false,
                receiver_awaited: false,
                receiver_local: false,
                receiver_lambda: None,
            })
            .collect(),
        names: facts
            .names
            .into_iter()
            .map(|n| FragName {
                name: n.name,
                kind: n.kind,
                line: n.line,
                owner: n.owner,
            })
            .collect(),
    })
}
