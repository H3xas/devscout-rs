use super::*;

// Real C# through this crate's own extractor, so a fact that never gets
// recorded fails here rather than passing vacuously.

// A receiver DECLARED with a generic-final qualified type reaches the ladder's
// exact-qualified step with its type-argument list still attached, and no def
// id carries a list. Every fixture below therefore puts an `App`-rooted and a
// `Lib`-rooted twin at the SAME written tail, so the enclosing-namespace walk
// is the only step that can answer and the dotted-suffix step can only answer
// ambiguous -- which is what makes a lost enclosing-namespace decision
// observable at all.

fn line_of(source: &str, marker: &str) -> usize {
    source
        .lines()
        .position(|line| line.contains(marker))
        .expect("fixture marker is present")
        + 1
}

/// One file's precise uses-member edges as (target id, line), sorted.
fn site_edges<'a>(g: &'a Graph, file: &str) -> Vec<(&'a str, usize)> {
    let mut edges = member_edges_from(g, file);
    edges.sort_unstable();
    edges
}

const BOX_TWINS: &[(&str, &str)] = &[
    (
        "App/Widgets/Box.cs",
        "namespace App.Widgets { public class Box<T> { public void Render() { } } }",
    ),
    (
        "App/Widgets/Pair.cs",
        "namespace App.Widgets { public class Pair<T1, T2> { public void Render() { } } }",
    ),
    (
        "App/Widgets/Gadget.cs",
        "namespace App.Widgets { public class Gadget { public void Render() { } } }",
    ),
    (
        "Lib/Widgets/Box.cs",
        "namespace Lib.Widgets { public class Box<T> { public void Render() { } } }",
    ),
    (
        "Lib/Widgets/Pair.cs",
        "namespace Lib.Widgets { public class Pair<T1, T2> { public void Render() { } } }",
    ),
    (
        "Lib/Widgets/Gadget.cs",
        "namespace Lib.Widgets { public class Gadget { public void Render() { } } }",
    ),
];

const SITE: &str = r#"
namespace App
{
    public class Site
    {
        public void Run(Widgets.Box<int> parameter)
        {
            Widgets.Box<int> local = null;
            Widgets.Box<int>? nullable = null;
            Widgets.Pair<int, string> pair = null;
            Widgets.Gadget gadget = null;
            local.Render();
            parameter.Render();
            nullable.Render();
            pair.Render();
            gadget.Render();
            if (parameter is Widgets.Box<int> pattern)
            {
                pattern.Render();
            }
        }
    }
}
"#;

fn site_graph() -> Graph {
    let mut files = fragments_for(BOX_TWINS);
    files.push((
        "App/Site.cs".to_string(),
        crate::graph::fragment_from_extraction(&crate::extract::extract(SITE)),
    ));
    resolve_graph(&no_git_root(), &files)
}

#[test]
fn generic_receiver_path_ac_1_site_declared_receivers_take_the_namespace_walk() {
    let g = site_graph();
    assert_eq!(
        site_edges(&g, "App/Site.cs"),
        vec![
            ("App.Widgets.Box", line_of(SITE, "local.Render()")),
            ("App.Widgets.Box", line_of(SITE, "parameter.Render()")),
            ("App.Widgets.Box", line_of(SITE, "nullable.Render()")),
            ("App.Widgets.Box", line_of(SITE, "pattern.Render()")),
            ("App.Widgets.Gadget", line_of(SITE, "gadget.Render()")),
            ("App.Widgets.Pair", line_of(SITE, "pair.Render()")),
        ],
        "every declared receiver binds the App-rooted twin, as the non-generic local does"
    );
}

#[test]
fn generic_receiver_path_ac_1_binds_precisely_and_never_the_lib_twin() {
    let g = site_graph();
    let tiers: Vec<Option<HeuristicTier>> = member_edges_with_file(&g, "App/Site.cs")
        .into_iter()
        .map(|(_, _, _, tier)| tier)
        .collect();
    assert!(
        tiers.iter().all(Option::is_none),
        "each read is precise -- the tier the non-generic local gets in the same fixture"
    );
    assert!(
        heuristic_member_edges_from(&g, "App/Site.cs").is_empty(),
        "no receiver here reaches a guess tier"
    );
    for twin in ["Lib.Widgets.Box", "Lib.Widgets.Pair", "Lib.Widgets.Gadget"] {
        assert!(
            !any_member_edge_targets(&g, twin),
            "the sibling root earns no edge at all, precise or guessed"
        );
    }
}

const OTHER_DECLARATION_SHAPES: &str = r#"
namespace App
{
    public class Site
    {
        public void Run(System.Collections.Generic.IEnumerable<Widgets.Box<int>> boxes)
        {
            foreach (Widgets.Box<int> item in boxes)
            {
                item.Render();
            }
            try
            {
                Widgets.Box<int> ignored = null;
            }
            catch (Widgets.Box<int> fault)
            {
                fault.Render();
            }
            using (Widgets.Box<int> scoped = null)
            {
                scoped.Render();
            }
            TryFill(out Widgets.Box<int> produced);
            produced.Render();
        }

        private static void TryFill(out Widgets.Box<int> value)
        {
            value = null;
        }
    }
}
"#;

#[test]
fn generic_receiver_path_ac_1_foreach_catch_using_and_out_receivers_take_the_walk_too() {
    let mut files = fragments_for(BOX_TWINS);
    files.push((
        "App/Site.cs".to_string(),
        crate::graph::fragment_from_extraction(&crate::extract::extract(OTHER_DECLARATION_SHAPES)),
    ));
    let g = resolve_graph(&no_git_root(), &files);
    let mut expected: Vec<(&str, usize)> = [
        "item.Render()",
        "fault.Render()",
        "scoped.Render()",
        "produced.Render()",
    ]
    .iter()
    .map(|marker| ("App.Widgets.Box", line_of(OTHER_DECLARATION_SHAPES, marker)))
    .collect();
    expected.sort_unstable();
    assert_eq!(
        site_edges(&g, "App/Site.cs"),
        expected,
        "every declaration a receiver may be written on reaches the same walk"
    );
}

const SIBLING_DECLARING: &str = r#"
namespace App
{
    public partial class Host
    {
        private Widgets.Box<int> _box;
    }
}
"#;

const SIBLING_READING: &str = r#"
namespace App
{
    public partial class Host
    {
        public void Run()
        {
            _box.Render();
        }
    }
}
"#;

const BASE_DECLARING: &str = r#"
namespace App
{
    public class Base
    {
        public Widgets.Box<int> Current { get; }
    }
}
"#;

const DERIVED_READING: &str = r#"
namespace App
{
    public class Derived : Base
    {
        public void Run()
        {
            Current.Render();
        }
    }
}
"#;

// The two fallbacks that reach the ladder with no recorded arity: a field
// merged in from a partial sibling file and a property read off a base class.
// Both carry the written path and nothing else, so the written list's own
// argument count is the only gate available to them.
#[test]
fn generic_receiver_path_ac_2_field_and_property_fallbacks_bind_the_namespace_walk() {
    let source =
        |text: &str| crate::graph::fragment_from_extraction(&crate::extract::extract(text));
    let mut files = fragments_for(BOX_TWINS);
    files.push((
        "App/Host.Declaring.cs".to_string(),
        source(SIBLING_DECLARING),
    ));
    files.push(("App/Host.Sibling.cs".to_string(), source(SIBLING_READING)));
    files.push(("App/Base.cs".to_string(), source(BASE_DECLARING)));
    files.push(("App/Derived.cs".to_string(), source(DERIVED_READING)));
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        site_edges(&g, "App/Host.Sibling.cs"),
        vec![("App.Widgets.Box", line_of(SIBLING_READING, "_box.Render()"))],
        "a field read from a partial sibling file binds the namespace walk"
    );
    assert_eq!(
        site_edges(&g, "App/Derived.cs"),
        vec![(
            "App.Widgets.Box",
            line_of(DERIVED_READING, "Current.Render()")
        )],
        "a property declared on a base class and read from a derived one binds it too"
    );
}

const BOX_PLAIN: (&str, &str) = (
    "App/Widgets/BoxPlain.cs",
    "namespace App.Widgets { public class Box { public void Render() { } } }",
);

const BOX_GENERIC: (&str, &str) = (
    "App/Widgets/BoxGeneric.cs",
    "namespace App.Widgets { public class Box<T> { public void Render() { } } }",
);

const ARITY_SITE: &str = r#"
namespace App
{
    public class Site
    {
        public void Run()
        {
            Widgets.Box<int> generic = null;
            Widgets.Box plain = null;
            Widgets.Box<int, string> unmatched = null;
            generic.Render();
            plain.Render();
            unmatched.Render();
        }
    }
}
"#;

const SHAPES_DECLARING: &str = r#"
namespace App
{
    public partial class Shapes
    {
        private Widgets.Box<int> _one;
        private Widgets.Box<List<int>> _nested;
        private Widgets.Box<(int, string)> _tuple;
        private Widgets.Box<int[,]> _array;
    }
}
"#;

const SHAPES_READING: &str = r#"
namespace App
{
    public partial class Shapes
    {
        public void Run()
        {
            _one.Render();
            _nested.Render();
            _tuple.Render();
            _array.Render();
        }
    }
}
"#;

/// The same fixture with the non-generic and the generic `Box` swapped in the
/// file order, so neither declaration order can decide which def an arity-blind
/// lookup would find.
fn arity_graph(plain_first: bool) -> Graph {
    let source =
        |text: &str| crate::graph::fragment_from_extraction(&crate::extract::extract(text));
    let order = if plain_first {
        [BOX_PLAIN, BOX_GENERIC]
    } else {
        [BOX_GENERIC, BOX_PLAIN]
    };
    let mut files: Vec<(String, Fragment)> = order
        .iter()
        .map(|(path, text)| ((*path).to_string(), source(text)))
        .collect();
    files.push(("Lib/Widgets/Box.cs".to_string(), source(BOX_TWINS[3].1)));
    files.push(("App/Site.cs".to_string(), source(ARITY_SITE)));
    files.push((
        "App/Shapes.Declaring.cs".to_string(),
        source(SHAPES_DECLARING),
    ));
    files.push(("App/Shapes.Reading.cs".to_string(), source(SHAPES_READING)));
    resolve_graph(&no_git_root(), &files)
}

/// One file's PRECISE member edges as (target id, target file, line) -- the
/// target file is what tells two defs sharing one id apart, and the tier filter
/// is what keeps a guessed edge out of a claim about binding.
fn edges_with_file<'a>(g: &'a Graph, file: &str) -> Vec<(&'a str, &'a str, usize)> {
    let mut edges: Vec<(&str, &str, usize)> = member_edges_with_file(g, file)
        .into_iter()
        .filter(|(_, _, _, tier)| tier.is_none())
        .map(|(to, line, to_file, _)| (to, to_file, line))
        .collect();
    edges.sort_unstable();
    edges
}

// `Box` and `Box<T>` share one id and one arity-blind lookup slot, so the
// count a lookup is keyed by is the whole difference between binding the def
// the declaration names and binding whichever sibling the index met first.
#[test]
fn generic_receiver_path_ac_3_the_declared_arity_selects_the_sibling() {
    for plain_first in [true, false] {
        let g = arity_graph(plain_first);
        assert_eq!(
            edges_with_file(&g, "App/Site.cs"),
            vec![
                (
                    "App.Widgets.Box",
                    "App/Widgets/BoxGeneric.cs",
                    line_of(ARITY_SITE, "generic.Render()")
                ),
                (
                    "App.Widgets.Box",
                    "App/Widgets/BoxPlain.cs",
                    line_of(ARITY_SITE, "plain.Render()")
                ),
            ],
            "`Box<int>` takes the one-parameter def and bare `Box` the arity-zero one, \
             whichever order the two were declared in"
        );
        assert!(
            !edges_with_file(&g, "App/Site.cs")
                .iter()
                .any(|(_, _, line)| *line == line_of(ARITY_SITE, "unmatched.Render()")),
            "no two-parameter `Box` is declared, so `Box<int, string>` earns no precise edge"
        );
    }
}

#[test]
fn generic_receiver_path_ac_3_the_written_count_gates_the_arity_less_fallback() {
    for plain_first in [true, false] {
        let g = arity_graph(plain_first);
        assert_eq!(
            edges_with_file(&g, "App/Shapes.Reading.cs")
                .into_iter()
                .map(|(to, to_file, line)| (to, to_file, line))
                .collect::<Vec<_>>(),
            [
                "_one.Render()",
                "_nested.Render()",
                "_tuple.Render()",
                "_array.Render()"
            ]
            .iter()
            .map(|marker| (
                "App.Widgets.Box",
                "App/Widgets/BoxGeneric.cs",
                line_of(SHAPES_READING, marker)
            ))
            .collect::<Vec<_>>(),
            "with no recorded arity the written count decides, and a nested list, a tuple \
             and an array each count one argument"
        );
    }
}

const QUALIFIED_TWINS: &[(&str, &str)] = &[BOX_TWINS[0], BOX_TWINS[3]];

fn qualified_receiver_ref(text: &str, args: Option<&[&str]>) -> FragRef {
    FragRef {
        receiver_type: Some("Box".into()),
        receiver_type_qualified: Some(text.into()),
        receiver_args: args.map(|list| list.iter().map(|a| (*a).to_string()).collect()),
        ..member_ref("box", None, "Render", "App")
    }
}

fn probe_graph(text: &str, args: Option<&[&str]>) -> Graph {
    let mut files = fragments_for(QUALIFIED_TWINS);
    files.push((
        "App/Site.cs".to_string(),
        frag(
            vec![def("App.Site", "Site", "App", "class")],
            vec![],
            vec![qualified_receiver_ref(text, args)],
        ),
    ));
    resolve_graph(&no_git_root(), &files)
}

// One catch-all refusal: a written path that is not exactly one well-formed
// list closing its final segment keeps step 1's verbatim lookup, so a shape
// the rule cannot read answers the way the base answers it. Every refused
// spelling below would reach `App.Widgets.Box` through the namespace walk if
// it were accepted, which is what makes "the base's answer" falsifiable.
#[test]
fn generic_receiver_path_ac_4_every_refused_shape_keeps_the_base_answer() {
    for (text, args) in [
        ("Widgets.Box<>", Some(&[][..])),
        ("Widgets.Box<int,>", Some(&["int"][..])),
        ("Widgets.Box<(int, string>", Some(&["*"][..])),
        ("Widgets.Box<int>>", Some(&["int"][..])),
        ("Widgets.Box<int]>", Some(&["int"][..])),
        ("Widgets.Box<[int", Some(&["int"][..])),
        ("global::App.Widgets.Box<int>", None),
    ] {
        assert_eq!(
            member_edge_targets(&probe_graph(text, args)),
            Vec::<&str>::new(),
            "`{text}` is not one well-formed list closing the final segment, so step 1 \
             keeps the verbatim lookup the base answers with"
        );
    }
}

// Every index the list is read at is an ASCII delimiter byte, so no slice can
// land inside a multi-byte character, and a well-formed list longer than any
// decl it could name still only fails to match.
#[test]
fn generic_receiver_path_ac_4_a_non_ascii_or_over_long_path_answers_empty() {
    for (text, args) in [
        ("Wîdgets.Box<int>", Some(&["int"][..])),
        ("Widgets.Box<int, string, bool>", Some(&["*", "*", "*"][..])),
    ] {
        assert_eq!(
            member_edge_targets(&probe_graph(text, args)),
            Vec::<&str>::new(),
            "`{text}` names nothing this graph declares"
        );
    }
}

#[test]
fn generic_receiver_path_ac_3_the_recorded_arity_outranks_the_written_count() {
    assert_eq!(
        member_edge_targets(&probe_graph("Widgets.Box<int>", Some(&["int"]))),
        vec!["App.Widgets.Box"],
        "the control the three cases below are read against: one well-formed list \
         closing the final segment takes the namespace walk"
    );
    assert_eq!(
        member_edge_targets(&probe_graph("Widgets.Box<int>", Some(&[]))),
        vec!["App.Widgets.Box"],
        "a recorded zero is the extractor's sentinel for an argument shape it could not \
         read, so the written list's own count decides"
    );
    assert_eq!(
        member_edge_targets(&probe_graph("Widgets.Box<int>", Some(&["*", "*"]))),
        Vec::<&str>::new(),
        "a probe recorded with two arguments is never answered by a one-parameter def"
    );
    assert_eq!(
        member_edge_targets(&probe_graph("Widgets.Box<int, string>", Some(&["*", "*"]))),
        Vec::<&str>::new(),
        "and no two-parameter `Box` is declared for it to reach"
    );
}

const NESTED_DECLARED: &[(&str, &str)] = &[
    (
        "App/Widgets/Outer.cs",
        "namespace App.Widgets { public class Outer<T> { public class Inner { public void Render() { } } } }",
    ),
    (
        "App/Widgets/Outer/Inner.cs",
        "namespace App.Widgets.Outer { public class Inner { public void Render() { } } }",
    ),
];

const NESTED_SITE: &str = r#"
namespace App
{
    public class Site
    {
        public void Run(Widgets.Outer<int>.Inner inner)
        {
            inner.Render();
        }
    }
}
"#;

// A list on an earlier segment belongs to a nested type's CONTAINER, and def
// ids join nesting with `+`, so a `.`-joined candidate can never name one.
// Stripping that list instead would let the NAMESPACE-level
// `App.Widgets.Outer.Inner` answer for the nested `App.Widgets.Outer+Inner`;
// both are declared here so the wrong lookup would find exactly the wrong one.
#[test]
fn generic_receiver_path_ac_4_a_list_on_an_earlier_segment_keeps_the_verbatim_lookup() {
    let mut files = fragments_for(NESTED_DECLARED);
    files.push((
        "App/Site.cs".to_string(),
        crate::graph::fragment_from_extraction(&crate::extract::extract(NESTED_SITE)),
    ));
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        site_edges(&g, "App/Site.cs"),
        Vec::<(&str, usize)>::new(),
        "the non-final-segment list is not stripped into a lookup that finds the \
         namespace-level `Inner`"
    );
}

const ALIAS_SITE: &str = r#"
namespace App
{
    public class Site
    {
        public void Run(global::App.Widgets.Box<int> box)
        {
            box.Render();
        }
    }
}
"#;

// `global::` is a qualifier rather than a segment, so a path carrying one is
// never a final-segment list however its tail reads. The alias-aware steps
// answer it exactly as before.
#[test]
fn generic_receiver_path_ac_4_an_alias_qualified_receiver_keeps_its_answer() {
    let mut files = fragments_for(QUALIFIED_TWINS);
    files.push((
        "App/Site.cs".to_string(),
        crate::graph::fragment_from_extraction(&crate::extract::extract(ALIAS_SITE)),
    ));
    let g = resolve_graph(&no_git_root(), &files);
    assert_eq!(
        site_edges(&g, "App/Site.cs"),
        vec![("App.Widgets.Box", line_of(ALIAS_SITE, "box.Render()"))],
        "the alias qualifier is dropped by the alias-aware step, not by this rule"
    );
}
