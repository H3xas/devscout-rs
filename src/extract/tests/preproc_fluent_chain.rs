use super::*;

// --- `#if`/`#if-else` interrupting a fluent chain ------------------
//
// Each fixture below has a conditional-compilation group sitting in the
// middle of a fluent member-access/invocation chain. Before parsing,
// `preproc::strip_inactive` blanks every inactive arm -- and the
// directive lines themselves -- with spaces, so by the time the parser
// sees the file the chain reads as one uninterrupted statement running
// straight from the call before the group into the call after it.
//
// The fixtures live in fixtures/preproc/ and are `include_str!`ed here,
// so the assertions below are pinned to the exact same bytes a real
// extraction would see; a line number changing under them means the
// fixture drifted, not that the extractor's behavior changed.

pub(super) const IF_DIRECTIVE_CHAIN: &str =
    include_str!("../../../fixtures/preproc/preproc_chain_interrupt_if.cs");
const IFELSE_DIRECTIVE_CHAIN: &str =
    include_str!("../../../fixtures/preproc/preproc_chain_interrupt_ifelse.cs");
const NESTED_IF_DIRECTIVE_CHAIN_CONTROL: &str =
    include_str!("../../../fixtures/preproc/preproc_chain_interrupt_nested_control.cs");
const WHOLESTMT_DIRECTIVE_CONTROL: &str =
    include_str!("../../../fixtures/preproc/preproc_chain_wholestmt_control.cs");

fn uses_member_refs(e: &Extraction) -> Vec<(&str, &str, usize)> {
    e.refs
        .iter()
        .filter(|r| r.kind == "uses-member")
        .map(|r| (r.name.as_str(), r.member.as_deref().unwrap_or(""), r.line))
        .collect()
}

#[test]
fn preproc_if_arm_inside_a_chain_is_not_indexed_and_the_chain_continues() {
    let e = extract_src(IF_DIRECTIVE_CHAIN);
    let refs = uses_member_refs(&e);
    // `DEBUG` is undefined, so the `#if` arm -- `.WriteTo.Debug()` on
    // line 18 -- is blanked before parsing and contributes no ref at
    // all: neither "Debug" as a member name...
    assert!(!refs.iter().any(|(_, member, _)| *member == "Debug"));
    // ...nor "WriteTo" as a qualifier (the fluent chain itself is
    // rooted in a `new Pipeline()`, so none of its own `.Enrich`,
    // `.Filter`, `.WriteTo` or `.MinimumLevel` steps ever produce a
    // ref -- only their call arguments do, which is exactly the shape
    // asserted below).
    assert!(!refs.iter().any(|(name, _, _)| *name == "WriteTo"));
    // The chain parses as a single statement spanning the blanked
    // lines, so every call argument keeps its real line number and the
    // whole array stays in ascending line order.
    assert_eq!(
        refs,
        vec![
            ("e", "Level", 9),
            ("Level", "Error", 9),
            ("Interval", "Day", 13),
            ("Level", "Information", 20),
            ("Level", "Information", 21),
        ]
    );
}

#[test]
fn preproc_ifelse_group_drops_the_dead_arm_and_leaves_the_chain_tail_intact() {
    let e = extract_src(IFELSE_DIRECTIVE_CHAIN);
    let refs = uses_member_refs(&e);
    // `TRACE` is undefined, so the `#if` arm's `.WriteTo.Trace()` (line
    // 13) is blanked before parsing. The `#else` arm's
    // `.WriteTo.Console()` (line 15) survives the pre-pass and keeps
    // the chain intact, but -- like every other fluent qualifier on
    // this `new Pipeline()` chain -- neither call produces a ref of
    // its own; only the trailing `.MinimumLevel.Override(...)`
    // argument does.
    assert!(!refs.iter().any(|(_, member, _)| *member == "Trace"));
    assert!(!refs.iter().any(|(_, member, _)| *member == "Console"));
    assert_eq!(refs, vec![("Level", "Information", 17)]);
}

#[test]
fn preproc_nested_if_in_if_leaves_the_whole_inner_group_absent() {
    // The outer `#if DEBUG` is undefined, so the entire group -- the
    // nested `#if TRACE`/`#endif` and both `.WriteTo.Trace()` (line 14)
    // and `.WriteTo.Debug()` (line 16) -- is blanked before parsing,
    // regardless of the inner symbol. Only the trailing
    // `.MinimumLevel.Override(...)` argument after the group remains.
    let e = extract_src(NESTED_IF_DIRECTIVE_CHAIN_CONTROL);
    let refs = uses_member_refs(&e);
    assert!(!refs.iter().any(|(_, member, _)| *member == "Trace"));
    assert!(!refs.iter().any(|(_, member, _)| *member == "Debug"));
    assert_eq!(refs, vec![("Level", "Information", 18)]);
}

#[test]
fn preproc_if_wrapping_a_whole_statement_removes_only_the_guarded_call() {
    // An ordinary statement-level `#if DEBUG { ... }` guards a whole
    // call rather than interrupting an expression. `DEBUG` is
    // undefined, so the guarded call -- `registry.Attach(GetDebugSink())`
    // on line 9 -- is blanked before parsing and produces no ref; the
    // unguarded call on line 7 is untouched.
    let e = extract_src(WHOLESTMT_DIRECTIVE_CONTROL);
    let refs = uses_member_refs(&e);
    assert_eq!(refs, vec![("registry", "Attach", 7)]);
}

const NAMESPACE_SELECTION_SRC: &str =
    include_str!("../../../fixtures/preproc/preproc_namespace_selection.cs");

#[test]
fn preproc_namespace_selection_indexes_only_the_active_arm() {
    let e = extract_src(NAMESPACE_SELECTION_SRC);

    // The `#if` arm's namespace (`Fixtures.Preproc.LightLattice`) is
    // undefined, so every type is indexed exactly once, under the
    // `#else` arm's namespace only.
    assert_eq!(e.defs.len(), 7);
    let expected: Vec<(&str, &str, usize)> = vec![
        ("Fixtures.Preproc.Lattice.LatticeCompiler", "class", 9),
        (
            "Fixtures.Preproc.Lattice.LatticeCompiler+EmitMode",
            "enum",
            18,
        ),
        (
            "Fixtures.Preproc.Lattice.LatticeCompiler+EmitMode.Direct",
            "enum-member",
            20,
        ),
        (
            "Fixtures.Preproc.Lattice.LatticeCompiler+EmitMode.Delegated",
            "enum-member",
            21,
        ),
        ("Fixtures.Preproc.Lattice.LatticeEmitter", "class", 25),
        ("Fixtures.Preproc.Lattice.Closure", "class", 33),
        ("Fixtures.Preproc.Lattice.Expression", "class", 38),
    ];
    for (id, kind, line) in &expected {
        let d = find_def(&e, id).unwrap_or_else(|| panic!("def {id} present"));
        assert_eq!(d.kind, *kind, "{id} kind");
        assert_eq!(d.line, *line, "{id} line");
        assert_eq!(d.namespace, "Fixtures.Preproc.Lattice", "{id} namespace");
    }

    assert!(e.defs.iter().all(|d| !d.id.contains("LightLattice")));

    assert_eq!(e.usings.len(), 1);
    match &e.usings[0] {
        UsingRecord::Plain { text, global } => {
            assert_eq!(text, "Fixtures.Preproc.Lattice.Expression");
            assert!(!global);
        }
        UsingRecord::Alias { .. } => panic!("expected plain form"),
    }

    assert!(!e.refs.iter().any(|r| r.name.contains("LightLattice")
        || r.qualified
            .as_deref()
            .unwrap_or("")
            .contains("LightLattice")));
}
