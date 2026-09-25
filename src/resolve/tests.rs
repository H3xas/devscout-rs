use super::index::build_def_index;
use super::{resolve_graph, resolve_graph_with_model};
use crate::graph::FragDef;
use crate::graph::{
    AlsoIn, Edge, FragExtensionMethod, FragFact, FragRef, FragUsing, Fragment, Graph,
    HeuristicByTier, HeuristicTier, OrderedMap, Percent1, GRAPH_SCHEMA_VERSION,
};
use crate::manifest;

mod base_members;
mod base_walk;
mod bus;
mod byte_identity;
mod ctor_di;
mod dispatch;
mod dotted_suffix;
mod extension_generics;
mod extension_tier;
mod graph_invariants;
mod ladder;
mod lambda_parameters;
mod member_qualifiers;
mod nested_end_to_end;
mod nested_step;
mod project_admission;
mod qualified_names;
mod receiver_hops;
mod receiver_rule;
mod receiver_tier;
mod scored_tier;

/// Not a real git repo -- `resolve_graph`'s single I/O call (`git_head`)
/// fails closed to `None` here, the same as a non-repo shell-out failure.
fn no_git_root() -> std::path::PathBuf {
    std::env::temp_dir().join("scout-resolve-test-not-a-repo")
}

fn def(id: &str, name: &str, ns: &str, kind: &str) -> FragDef {
    FragDef {
        id: id.into(),
        name: name.into(),
        namespace: ns.into(),
        kind: kind.into(),
        line: 1,
        methods: vec![],
        properties: vec![],
        fields: vec![],
        method_returns: crate::graph::OrderedMap::new(),
        extension_methods: vec![],
        bases: vec![],
        type_params: vec![],
        base_generic_args: crate::graph::OrderedMap::new(),
        test_methods: vec![],
        property_types: crate::graph::OrderedMap::new(),
        field_types: crate::graph::OrderedMap::new(),
        method_return_args: crate::graph::OrderedMap::new(),
        non_public_methods: vec![],
        method_arities: crate::graph::OrderedMap::new(),
        method_params: crate::graph::OrderedMap::new(),
        override_methods: vec![],
        base_type_args: crate::graph::OrderedMap::new(),
        property_message_args: Vec::new(),
        array_message_bases: Vec::new(),
        end_line: 0,
    }
}

/// `def()` for a static class declaring extension methods -- the only input
/// tier (f) reads besides the def's own namespace. The tuples are (name,
/// this_type, arity_min, arity_max).
fn ext_def(id: &str, name: &str, ns: &str, extensions: &[(&str, &str, usize, i64)]) -> FragDef {
    FragDef {
        extension_methods: extensions
            .iter()
            .map(|(n, t, lo, hi)| FragExtensionMethod {
                name: (*n).to_string(),
                this_type: (*t).to_string(),
                arity_min: *lo,
                arity_max: *hi,
                this_args: None,
            })
            .collect(),
        ..def(id, name, ns, "class")
    }
}

/// `def()` with member lists filled in -- the resolver reads
/// `methods`/`properties`/`fields` as one union (tier (a)), so every tier
/// test that turns on WHICH list a member lives in builds its def here.
fn def_with(
    id: &str,
    name: &str,
    ns: &str,
    kind: &str,
    methods: &[&str],
    properties: &[&str],
    fields: &[&str],
) -> FragDef {
    FragDef {
        methods: methods.iter().map(|s| s.to_string()).collect(),
        properties: properties.iter().map(|s| s.to_string()).collect(),
        fields: fields.iter().map(|s| s.to_string()).collect(),
        ..def(id, name, ns, kind)
    }
}

fn type_ref(kind: &str, name: &str, qualified: Option<&str>, ns: &str) -> FragRef {
    FragRef {
        kind: kind.into(),
        name: name.into(),
        qualified: qualified.map(String::from),
        member: None,
        line: 1,
        namespace: Some(ns.into()),
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
        receiver_nullable: false,
        lambda_arg_arity: None,
    }
}

fn member_ref(name: &str, qualified: Option<&str>, member: &str, ns: &str) -> FragRef {
    FragRef {
        kind: "uses-member".into(),
        name: name.into(),
        qualified: qualified.map(String::from),
        member: Some(member.into()),
        line: 1,
        namespace: Some(ns.into()),
        type_arg_count: None,
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
        receiver_nullable: false,
        lambda_arg_arity: None,
    }
}

/// A bare-qualifier member ref carrying the receiver fact the extractor
/// would have recorded for it. `arg_count` is the CALL shape: `Some(n)` for
/// `x.M(<n args>)`, `None` for a property read.
fn receiver_ref(
    name: &str,
    member: &str,
    ns: &str,
    receiver_type: &str,
    arg_count: Option<usize>,
) -> FragRef {
    FragRef {
        receiver_type: Some(receiver_type.into()),
        arg_count,
        ..member_ref(name, None, member, ns)
    }
}

fn frag(defs: Vec<FragDef>, usings: Vec<FragUsing>, refs: Vec<FragRef>) -> Fragment {
    Fragment {
        defs,
        usings,
        refs,
        names: Vec::new(),
        registrations: Vec::new(),
        publishes: Vec::new(),
        handler_registrations: Vec::new(),
    }
}

fn find_edge<'a>(g: &'a Graph, want: impl Fn(&Edge) -> bool) -> Option<&'a Edge> {
    g.edges.iter().find(|e| want(e))
}

// --- built_at_head threading (manifest::git_head unit-tested there;
// this is the integration check that resolve_graph actually calls it
// and plumbs the result into the right field) --------------------------

// The uses-member edge set is split in two. Almost every assertion in this
// module is about the PRECISE half, so the two default accessors below
// filter to it and the `heuristic_*` counterparts are what a heuristic-tier
// test reaches for -- rather than filtering `heuristic` inline forty times.
fn member_edge_targets(g: &Graph) -> Vec<&str> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                to,
                heuristic: false,
                ..
            } => Some(to.as_str()),
            _ => None,
        })
        .collect()
}

fn heuristic_member_edge_targets(g: &Graph) -> Vec<&str> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                to,
                heuristic: true,
                ..
            } => Some(to.as_str()),
            _ => None,
        })
        .collect()
}

fn fragments_for(files: &[(&str, &str)]) -> Vec<(String, Fragment)> {
    files
        .iter()
        .map(|(rel, src)| {
            (
                (*rel).to_string(),
                crate::graph::fragment_from_extraction(&crate::extract::extract(src)),
            )
        })
        .collect()
}

fn member_edges_from<'a>(g: &'a Graph, from: &str) -> Vec<(&'a str, usize)> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                from_line,
                to,
                heuristic: false,
                ..
            } if from_file == from => Some((to.as_str(), *from_line)),
            _ => None,
        })
        .collect()
}

/// The guessed half of the same file's uses-member edges, in emission order
/// (which for the scored tier IS scored order). Resolved TYPE-reference
/// targets out of one file, in edge order -- the ladder-walk tests assert on
/// these the way the member tests assert on `member_edges_from`.
fn type_edge_targets_from<'a>(g: &'a Graph, from: &str) -> Vec<&'a str> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesType { from_file, to, .. } if from_file == from => Some(to.as_str()),
            _ => None,
        })
        .collect()
}

fn heuristic_member_edges_from<'a>(g: &'a Graph, from: &str) -> Vec<(&'a str, usize)> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                from_line,
                to,
                heuristic: true,
                ..
            } if from_file == from => Some((to.as_str(), *from_line)),
            _ => None,
        })
        .collect()
}

/// The members named by one file's heuristic uses-member edges, in edge
/// order -- the fact `heuristic_member_edges_from` above cannot show.
fn heuristic_member_names_from<'a>(g: &'a Graph, from: &str) -> Vec<Option<&'a str>> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                heuristic: true,
                member,
                ..
            } if from_file == from => Some(member.as_deref()),
            _ => None,
        })
        .collect()
}

/// `true` when some uses-member edge out of `g`, precise or heuristic,
/// names `target` as its `to`. Used by the nested-qualifier chain tests
/// below to prove the outer container of a walked chain earns no edge at
/// all, not merely no PRECISE one -- `member_edges_from` and
/// `heuristic_member_edges_from` only ever show what DID emit.
fn any_member_edge_targets(g: &Graph, target: &str) -> bool {
    g.edges.iter().any(|e| match e {
        Edge::UsesMember { to, .. } => to == target,
        _ => false,
    })
}

const WIDGET_SRC: (&str, &str) = (
    "Other/Widget.cs",
    "namespace App.Other { public class Widget { } }",
);
const WIDGET_EXTENSIONS_SRC: (&str, &str) = (
    "Ext/WidgetExtensions.cs",
    "namespace App.Ext { public static class WidgetExtensions { public static void Render(this Widget w) { } } }",
);

/// Every `UsesMember` edge from one file, as (target id, line, target
/// file, tier) -- the target file is what lets the arity-aware receiver
/// tests below tell the generic sibling of a type apart from the
/// non-generic one sharing its id.
fn member_edges_with_file<'a>(
    g: &'a Graph,
    from: &str,
) -> Vec<(&'a str, usize, &'a str, Option<HeuristicTier>)> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                from_line,
                to,
                to_file,
                tier,
                ..
            } if from_file == from => Some((to.as_str(), *from_line, to_file.as_str(), *tier)),
            _ => None,
        })
        .collect()
}

// The byte-identity fixture: a fixed set of sources whose resolved edge and
// stats bytes are pinned exactly.
const BYTE_IDENTITY_FIXTURE: &[(&str, &str)] = &[
    ("Core/Status.cs", "\nnamespace App.Core;\n\npublic enum Status\n{\n  Active,\n  Idle\n}\n"),
    ("Core/IWidget.cs", "namespace App.Core { public interface IWidget { void Render(); } }"),
    (
        "Core/Widget.cs",
        "\nusing App.Core;\n\nnamespace App.Core;\n\npublic class Widget : IWidget\n{\n  public string Name { get; set; }\n\n  public void Render() { }\n}\n",
    ),
    ("Alpha/Config.cs", "namespace App.Alpha { public class Config { public void Load() { } } }"),
    ("Beta/Config.cs", "namespace App.Beta { public class Config { public void Load() { } } }"),
    (
        "Consumers/Consumer.cs",
        "\nusing App.Core;\nusing App.Alpha;\nusing App.Beta;\n\nnamespace App.Consumers;\n\npublic class Consumer\n{\n  private Widget _widget;\n\n  public void Run()\n  {\n    _widget.Render();\n    var s = Status.Active;\n    Config.Load();\n    var c = Compute();\n    c.Tally();\n  }\n}\n",
    ),
    ("Solo/Counter.cs", "namespace App.Solo { public class Counter { public void Tally() { } } }"),
];

// The precise-only bytes of that fixture's edge array (the heuristic edges
// dropped). A literal on purpose: a golden recomputed by the code under test
// proves nothing.
const PRE_STAGE4_EDGE_ROWS: &[&str] = &[
    r#"{"kind":"imports","from_file":"Core/Widget.cs","from_line":2,"target":"App.Core"}"#,
    r#"{"kind":"inherits","from_file":"Core/Widget.cs","from_line":6,"to":"App.Core.IWidget","to_file":"Core/IWidget.cs"}"#,
    r#"{"kind":"imports","from_file":"Consumers/Consumer.cs","from_line":2,"target":"App.Core"}"#,
    r#"{"kind":"imports","from_file":"Consumers/Consumer.cs","from_line":3,"target":"App.Alpha"}"#,
    r#"{"kind":"imports","from_file":"Consumers/Consumer.cs","from_line":4,"target":"App.Beta"}"#,
    r#"{"kind":"uses-type","from_file":"Consumers/Consumer.cs","from_line":10,"to":"App.Core.Widget","to_file":"Core/Widget.cs"}"#,
    r#"{"kind":"uses-member","from_file":"Consumers/Consumer.cs","from_line":14,"to":"App.Core.Widget","to_file":"Core/Widget.cs","member":"Render"}"#,
    r#"{"kind":"uses-member","from_file":"Consumers/Consumer.cs","from_line":15,"to":"App.Core.Status.Active","to_file":"Core/Status.cs","member":"Active"}"#,
];

/// One hand-built `Unit`: `id` is the repo-relative `.csproj` path, `dir`
/// is derived from it exactly as discovery derives it, `name` is the file
/// stem, and `refs` are the ids this project references DIRECTLY (the
/// model closes over them).
fn unit(id: &str, refs: &[&str], test: bool) -> crate::project::Unit {
    let (dir, file) = match id.rfind('/') {
        Some(i) => (&id[..i], &id[i + 1..]),
        None => ("", id),
    };
    crate::project::Unit {
        id: id.to_string(),
        name: file.trim_end_matches(".csproj").to_string(),
        dir: dir.to_string(),
        refs: refs.iter().map(|r| (*r).to_string()).collect(),
        test,
    }
}

fn model_of(units: Vec<crate::project::Unit>) -> crate::project::ProjectModel {
    crate::project::ProjectModel::from_units(units)
}

/// The tiers carried by one file's heuristic uses-member edges, in edge
/// order -- what `heuristic_member_edges_from` cannot show.
fn heuristic_member_tiers_from(g: &Graph, from: &str) -> Vec<Option<HeuristicTier>> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                heuristic: true,
                tier,
                ..
            } if from_file == from => Some(*tier),
            _ => None,
        })
        .collect()
}

// Two same-named `Config` classes, so the ladder's global-uniqueness step
// cannot answer `Config` on its own and the `global using` is the ONLY
// thing that can pick one -- which is what makes "who can see that global
// using" observable at all.
const SCOPED_GLOBAL_USING_FIXTURE: &[(&str, &str)] = &[
    (
        "src/Alpha/Config.cs",
        "namespace Fixture.Alpha { public class Config { public void Load() { } } }",
    ),
    (
        "src/Beta/Config.cs",
        "namespace Fixture.Beta { public class Config { public void Load() { } } }",
    ),
    ("src/App/GlobalUsings.cs", "global using Fixture.Alpha;\n"),
    (
        "src/App/AppConsumer.cs",
        "\nnamespace Fixture.App;\n\npublic class AppConsumer\n{\n  public void Run() => Config.Load();\n}\n",
    ),
    (
        "src/Other/OtherConsumer.cs",
        "\nnamespace Fixture.Other;\n\npublic class OtherConsumer\n{\n  public void Run() => Config.Load();\n}\n",
    ),
];

// Two same-named `Config` classes in two different projects and one
// consumer that names `Config` twice: once as a plain type reference (the
// field declaration on line 6) and once as a uses-member qualifier
// (`Config.Load()` on line 8). No using is in scope, so both refs are
// answered at the ladder's global-simple-name step, where two candidates
// is exactly an ambiguity -- so one resolve shows what the model does to
// both consumers at once.
const CROSS_PROJECT_AMBIGUITY_FIXTURE: &[(&str, &str)] = &[
    (
        "src/Alpha/Config.cs",
        "namespace Fixture.Alpha { public class Config { public void Load() { } } }",
    ),
    (
        "src/Beta/Config.cs",
        "namespace Fixture.Beta { public class Config { public void Load() { } } }",
    ),
    (
        "src/App/Runner.cs",
        "\nnamespace Fixture.App;\n\npublic class Runner\n{\n  private Config _config;\n\n  public void Run() => Config.Load();\n}\n",
    ),
];

/// Every `ambiguous` edge out of one file as (origin, raw, candidate ids,
/// `candidate_count`) -- the capped list AND the uncapped total, since
/// narrowing has to shrink both or neither.
fn ambiguous_edges_from<'a>(
    g: &'a Graph,
    from: &str,
) -> Vec<(&'a str, &'a str, Vec<&'a str>, usize)> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::Ambiguous {
                origin,
                from_file,
                raw,
                candidates,
                candidate_count,
                ..
            } if from_file == from => Some((
                origin.as_str(),
                raw.as_str(),
                candidates.iter().map(|c| c.id.as_str()).collect(),
                *candidate_count,
            )),
            _ => None,
        })
        .collect()
}

// The three-tier fixture: one file whose three member references are
// claimed by three different tiers, so a single resolve exercises the whole
// schema. `_widget.Render()` is a precise field hop, `_widget.Tally()` is
// an extension call only tier (f) can claim, and `Config.Load()` is
// ambiguous between two imported namespaces and reaches the scored tier.
const THREE_TIER_FIXTURE: &[(&str, &str)] = &[
    ("Core/Widget.cs", "namespace App.Core { public class Widget { public void Render() { } } }"),
    (
        "Ext/WidgetExtensions.cs",
        "namespace App.Ext { public static class WidgetExtensions { public static void Tally(this Widget w) { } } }",
    ),
    ("Alpha/Config.cs", "namespace App.Alpha { public class Config { public void Load() { } } }"),
    ("Beta/Config.cs", "namespace App.Beta { public class Config { public void Load() { } } }"),
    (
        "Consumers/Consumer.cs",
        "\nusing App.Core;\nusing App.Ext;\nusing App.Alpha;\nusing App.Beta;\n\nnamespace App.Consumers;\n\npublic class Consumer\n{\n  private Widget _widget;\n\n  public void Run()\n  {\n    _widget.Render();\n    _widget.Tally();\n    Config.Load();\n  }\n}\n",
    ),
];

/// `def()` carrying a test-method list -- the one member fact that reaches
/// graph.json's def rows.
fn test_def(id: &str, name: &str, ns: &str, test_methods: &[&str]) -> FragDef {
    FragDef {
        test_methods: test_methods.iter().map(|s| s.to_string()).collect(),
        ..def(id, name, ns, "class")
    }
}

/// A bare type ref carrying the enclosing-type stack the extractor would
/// have recorded for it.
fn nested_ref(kind: &str, name: &str, ns: &str, outer: &[&str]) -> FragRef {
    FragRef {
        outer_types: outer.iter().map(|s| (*s).to_string()).collect(),
        ..type_ref(kind, name, None, ns)
    }
}

fn def_with_bases_and_generics(
    id: &str,
    name: &str,
    ns: &str,
    bases: &[&str],
    type_params: &[&str],
    base_generic_args: &[(&str, &[&str])],
) -> FragDef {
    let mut bga = crate::graph::OrderedMap::new();
    for (k, v) in base_generic_args {
        bga.insert((*k).to_string(), v.iter().map(|s| s.to_string()).collect());
    }
    FragDef {
        bases: bases.iter().map(|s| s.to_string()).collect(),
        type_params: type_params.iter().map(|s| s.to_string()).collect(),
        base_generic_args: bga,
        ..def(id, name, ns, "class")
    }
}

fn ctor_param_ref(name: &str, ns: &str, args: Option<Vec<String>>) -> FragRef {
    FragRef {
        args,
        ..type_ref("ctor-param", name, None, ns)
    }
}

fn ctor_di_edges<'a>(g: &'a Graph, iface: &str) -> Vec<&'a Edge> {
    g.edges
        .iter()
        .filter(|e| matches!(e, Edge::CtorDi { iface: i, .. } if i == iface))
        .collect()
}

/// The two member->type maps the hops read, on top of `def_with`'s member
/// lists: (method, return type) and (property, declared type).
fn with_member_types(
    base: FragDef,
    method_returns: &[(&str, &str)],
    property_types: &[(&str, &str)],
) -> FragDef {
    let mut returns = OrderedMap::new();
    for (name, ty) in method_returns {
        returns.insert((*name).to_string(), (*ty).to_string());
    }
    let mut properties = OrderedMap::new();
    for (name, ty) in property_types {
        properties.insert(
            (*name).to_string(),
            FragFact {
                type_name: (*ty).to_string(),
                args: None,
            },
        );
    }
    FragDef {
        method_returns: returns,
        property_types: properties,
        ..base
    }
}

/// The TAIL window of a two-segment chain, carrying the head type the
/// extractor recorded for it: `head.<property>.<member>()`.
fn property_hop_ref(owner: &str, property: &str, member: &str, ns: &str) -> FragRef {
    FragRef {
        receiver_property_owner: Some(owner.into()),
        ..member_ref(property, Some(&format!("head.{property}")), member, ns)
    }
}

/// A bare-qualifier member ref whose qualifier is a
/// `var x = Owner.Callee(...)` local.
fn call_receiver_ref(name: &str, owner: &str, callee: &str, member: &str, ns: &str) -> FragRef {
    FragRef {
        receiver_call_owner: Some(owner.into()),
        receiver_call_member: Some(callee.into()),
        ..member_ref(name, None, member, ns)
    }
}

/// One file's PRECISE uses-member edges that name a given member, as
/// (target def id, line). The lambda-slot fixtures below all call the
/// callee ON THE SAME LINE as the lambda body, so filtering by target
/// alone cannot tell the two refs apart.
fn member_edges_named<'a>(g: &'a Graph, from: &str, member: &str) -> Vec<(&'a str, usize)> {
    g.edges
        .iter()
        .filter_map(|e| match e {
            Edge::UsesMember {
                from_file,
                from_line,
                to,
                member: m,
                heuristic: false,
                ..
            } if from_file == from && m.as_deref() == Some(member) => {
                Some((to.as_str(), *from_line))
            }
            _ => None,
        })
        .collect()
}
