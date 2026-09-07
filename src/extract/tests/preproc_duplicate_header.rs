use super::*;

// --- Duplicate-header preproc shape ---------------------------------
//
// `#if X <header> { ... #else <header> { ... #endif <shared tail> }`
// duplicates a member's header AND its opening brace across both arms
// while sharing one closing brace, so the raw text is unbalanced --
// reading both arms would leave two open braces and one close. The
// pre-pass blanks whichever arm is inactive, directive lines included,
// before the parser ever sees the file, so the parser is handed a
// single, well-formed header and body: no duplicated brace, no
// rebalancing to do.
//
// These assertions are the expected output for these exact strings.

// Two arms, one shared tail, one nested struct and one nested enum
// after it. The nested struct and enum keep their real scope, under
// the outer type, regardless of which arm the pre-pass keeps.
const DUPLICATE_HEADER_HOST_SRC: &str = "
namespace Fixtures.Preproc
{
    public static class WidgetCompiler
    {
        private static readonly int[] _pool = new int[8];

#if WIDGET_V2
        private static int[] ComputeOffsets(IWidgetProvider items)
        {
            var count = items.WidgetCount;
#else
        private static int[] ComputeOffsets(IReadOnlyList<Widget> items)
        {
            var count = items.Count;
#endif
            if (count == 0)
                return _pool;
            return _pool;
        }

        private struct SlotInfo
        {
            public object Payload;
        }

        [System.Flags]
        private enum SlotStatus : byte
        {
            Empty = 0,
            Filled = 1,
        }
    }
}
";

#[test]
fn duplicate_header_keeps_nested_type_defs_at_their_real_scope() {
    let e = extract_src(DUPLICATE_HEADER_HOST_SRC);

    assert!(find_def(&e, "Fixtures.Preproc.WidgetCompiler").is_some());

    let slot_info = find_def(&e, "Fixtures.Preproc.WidgetCompiler+SlotInfo")
        .expect("SlotInfo kept, nested under its type");
    assert_eq!(slot_info.kind, "struct");
    assert_eq!(slot_info.namespace, "Fixtures.Preproc");

    let slot_status =
        find_def(&e, "Fixtures.Preproc.WidgetCompiler+SlotStatus").expect("SlotStatus kept");
    assert_eq!(slot_status.kind, "enum");
    assert!(find_def(&e, "Fixtures.Preproc.WidgetCompiler+SlotStatus.Empty").is_some());
    assert!(find_def(&e, "Fixtures.Preproc.WidgetCompiler+SlotStatus.Filled").is_some());

    // `WIDGET_V2` is undefined, so the `#if` arm's parameter type and
    // the field access inside it are blanked before parsing and never
    // reach the refs...
    assert!(!e
        .refs
        .iter()
        .any(|r| r.kind == "uses-type" && r.name == "IWidgetProvider"));
    assert!(!e
        .refs
        .iter()
        .any(|r| r.member.as_deref() == Some("WidgetCount")));
    // ...while the `#else` arm's parameter type and field access are
    // present, untouched.
    assert!(e
        .refs
        .iter()
        .any(|r| r.kind == "uses-type" && r.name == "IReadOnlyList"));
    assert!(e
        .refs
        .iter()
        .any(|r| r.kind == "uses-type" && r.name == "Widget"));
    assert!(e
        .refs
        .iter()
        .any(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Count")));
}

#[test]
fn defs_and_refs_stay_in_ascending_line_order_when_an_if_group_is_blanked() {
    // Document order is the artifact contract; blanking an inactive arm
    // in place (rather than removing it) is where a line could most
    // easily drift out of order, so it is pinned here.
    let e = extract_src(DUPLICATE_HEADER_HOST_SRC);
    let def_lines: Vec<usize> = e.defs.iter().map(|d| d.line).collect();
    let mut sorted = def_lines.clone();
    sorted.sort();
    assert_eq!(
        def_lines, sorted,
        "defs must already be in ascending line order"
    );

    let ref_lines: Vec<usize> = e.refs.iter().map(|r| r.line).collect();
    let mut sorted_refs = ref_lines.clone();
    sorted_refs.sort();
    assert_eq!(
        ref_lines, sorted_refs,
        "refs must already be in ascending line order"
    );
}

#[test]
fn well_formed_file_is_unaffected() {
    let e = extract_src(
            "namespace Fixtures.Widgets { public class Gadget { public int Count; } public struct Handle {} }",
        );
    assert!(find_def(&e, "Fixtures.Widgets.Gadget").is_some());
    assert!(find_def(&e, "Fixtures.Widgets.Handle").is_some());
}

// Same shape, plus a SECOND nested type whose own body repeats the
// pattern one level deeper. Blanking each `#if` group independently
// keeps both wrapping types and both nested method bodies intact, no
// matter how many times the shape recurs in one file.
pub(super) const DUPLICATE_HEADER_HOST_WITH_NESTED_EMITTER_SRC: &str = "
namespace Fixtures.Preproc
{
    public static class WidgetCompiler
    {
        private static readonly int[] _pool = new int[8];

#if WIDGET_V2
        private static int[] ComputeOffsets(IWidgetProvider items)
        {
            var count = items.WidgetCount;
#else
        private static int[] ComputeOffsets(IReadOnlyList<Widget> items)
        {
            var count = items.Count;
#endif
            if (count == 0)
                return _pool;
            return _pool;
        }

        private struct SlotInfo
        {
            public object Payload;
        }

        [System.Flags]
        private enum SlotStatus : byte
        {
            Empty = 0,
            Filled = 1,
        }

        private static class Emitter
        {
#if WIDGET_V2
            public static bool TryEmit(Widget expr, IWidgetProvider items, Sink sink)
            {
                var scaled = items.WidgetCount;
#else
            public static bool TryEmit(Widget expr, IReadOnlyList<Widget> items, Sink sink)
            {
                var scaled = items.Count;
#endif
                sink.Write(scaled);
                return true;
            }
        }
    }
}
";

#[test]
fn stacked_duplicate_headers_keep_both_wrapping_types_at_their_real_scope() {
    let e = extract_src(DUPLICATE_HEADER_HOST_WITH_NESTED_EMITTER_SRC);

    assert!(find_def(&e, "Fixtures.Preproc.WidgetCompiler").is_some());
    let emitter = find_def(&e, "Fixtures.Preproc.WidgetCompiler+Emitter")
        .expect("Emitter kept, nested under WidgetCompiler");
    assert_eq!(emitter.kind, "class");
    assert_eq!(emitter.namespace, "Fixtures.Preproc");

    let slot_info = find_def(&e, "Fixtures.Preproc.WidgetCompiler+SlotInfo")
        .expect("SlotInfo kept, nested under its type");
    assert_eq!(slot_info.kind, "struct");
    assert!(find_def(&e, "Fixtures.Preproc.WidgetCompiler+SlotStatus").is_some());
    assert!(find_def(&e, "Fixtures.Preproc.WidgetCompiler+SlotStatus.Empty").is_some());
    assert!(find_def(&e, "Fixtures.Preproc.WidgetCompiler+SlotStatus.Filled").is_some());

    // The inner method's `#if` group is blanked the same way as the
    // outer one: no `IWidgetProvider` anywhere, and the refs inside the
    // active arm's body come through at their real nested scope.
    assert!(!e
        .refs
        .iter()
        .any(|r| r.kind == "uses-type" && r.name == "IWidgetProvider"));
    assert!(e
        .refs
        .iter()
        .any(|r| r.kind == "uses-type" && r.name == "Sink"));
    assert!(e.refs.iter().any(|r| r.kind == "uses-member"
        && r.name == "sink"
        && r.member.as_deref() == Some("Write")));
}
