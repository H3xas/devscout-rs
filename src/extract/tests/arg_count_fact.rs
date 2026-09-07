use super::super::json::{def_to_json, Json};
use super::preproc_fluent_chain::IF_DIRECTIVE_CHAIN;
use super::*;

// --- The ref-side `argCount` fact ------------------------------------------

// (qualified-or-bare qualifier + "." + member, argCount) for every
// uses-member ref, in extraction order.
fn member_arg_counts(e: &Extraction) -> Vec<(String, Option<usize>)> {
    e.refs
        .iter()
        .filter(|r| r.kind == "uses-member")
        .map(|r| {
            let qualifier = r.qualified.as_deref().unwrap_or(r.name.as_str());
            (
                format!("{qualifier}.{}", r.member.as_deref().unwrap_or("")),
                r.arg_count,
            )
        })
        .collect()
}

#[test]
fn stage3_arg_count_is_recorded_only_for_a_call_and_always_from_the_refs_own_invocation() {
    let e = extract_src(
        r#"
namespace App.Consumers;

public class Chain
{
  public void Run()
  {
    Widget w = new Widget();
    w.Inner.Tail(1, 2);
    Send(w.Payload);
    Send(w.Compute(7));
    var s = w.Slug;
  }
}
"#,
    );
    assert_eq!(
        member_arg_counts(&e),
        vec![
            // The flattened chain TAIL answers for ITS OWN call, not for
            // anything the head is part of -- the same independence stage 2
            // gave receiverType, now for the second borrowed-fact hazard.
            ("w.Inner.Tail".to_string(), Some(2)),
            // The chain HEAD is the qualifier of ".Tail", never a callee.
            ("w.Inner".to_string(), None),
            // A member access sitting in someone else's ARGUMENT list has an
            // invocation_expression above it too. It must NOT inherit that
            // call's count -- the guard is "this node IS the function
            // field", not "an invocation is somewhere overhead".
            ("w.Payload".to_string(), None),
            ("w.Compute".to_string(), Some(1)),
            // An ordinary property read: no argCount, which is what keeps it
            // out of the extension tier entirely.
            ("w.Slug".to_string(), None),
        ]
    );
}

#[test]
fn stage3_a_chain_ref_never_inherits_the_wrapping_calls_arg_count() {
    // With the inactive `#if DEBUG` arm blanked before parsing (see the
    // `#if`/`#if-else` fluent-chain tests above), every surviving
    // `uses-member` ref in this chain is a plain value read, not an
    // invocation, so none of them records an argCount. The load-bearing
    // row is `Interval.Day`: it sits inside the seven-argument
    // `.File(...)` argument list and must not inherit that call's count.
    let e = extract_src(IF_DIRECTIVE_CHAIN);
    assert_eq!(
        e.refs
            .iter()
            .filter(|r| r.kind == "uses-member")
            .map(|r| (
                r.name.as_str(),
                r.member.as_deref().unwrap_or(""),
                r.arg_count
            ))
            .collect::<Vec<_>>(),
        vec![
            ("e", "Level", None),
            ("Level", "Error", None),
            ("Interval", "Day", None),
            ("Level", "Information", None),
            ("Level", "Information", None),
        ]
    );
}

#[test]
fn stage3_a_type_declaring_no_extension_methods_records_an_empty_list() {
    let e = extract_src(
        "namespace App.Plain { public static class Utils { public static void Go(Widget w) { } } }",
    );
    let d = find_def(&e, "App.Plain.Utils").expect("Utils def present");
    assert_eq!(d.methods, vec!["Go"]);
    assert!(
        d.extension_methods.is_empty(),
        "empty means OMITTED at serialization -- pre-stage-3 bytes preserved"
    );
}

#[test]
fn stage3_this_type_unwraps_nullable_and_array_the_same_way_a_receiver_fact_does() {
    let e = extract_src(
        r#"
namespace App.Ext;

public static class Shapes
{
  public static void Each(this Widget[] items) { }
  public static void Maybe(this Widget? w) { }
  public static void Deep(this App.Other.Gadget g) { }
}
"#,
    );
    let d = find_def(&e, "App.Ext.Shapes").expect("Shapes def present");
    let pairs: Vec<(&str, &str)> = d
        .extension_methods
        .iter()
        .map(|x| (x.name.as_str(), x.this_type.as_str()))
        .collect();
    assert_eq!(
        pairs,
        vec![("Each", "Widget"), ("Maybe", "Widget"), ("Deep", "Gadget")]
    );
}

#[test]
fn stage3_a_this_parameter_carrying_a_second_modifier_is_still_an_extension_method() {
    // `this ref T` has TWO modifier children -- the scan is `.some(...)`,
    // not "the first modifier is `this`".
    let e = extract_src("namespace App.Ext { public static class R { public static void Bump(this ref Counter c) { } } }");
    let d = find_def(&e, "App.Ext.R").expect("R def present");
    let pairs: Vec<(&str, &str)> = d
        .extension_methods
        .iter()
        .map(|x| (x.name.as_str(), x.this_type.as_str()))
        .collect();
    assert_eq!(pairs, vec![("Bump", "Counter")]);
}

#[test]
fn stage3_base_type_identifier_keeps_predefined_only_for_the_this_parameter_caller() {
    // Same source, two fact families: `Trim`'s this-type records "string"
    // while the method's own predefined RETURN type still yields no
    // methodReturns fact -- the keep_predefined flag is per-call-site.
    let e = extract_src("namespace App.Ext { public static class S { public static string Trim(this string s) => s; } }");
    let d = find_def(&e, "App.Ext.S").expect("S def present");
    assert_eq!(
        d.extension_methods
            .iter()
            .map(|x| x.this_type.as_str())
            .collect::<Vec<_>>(),
        vec!["string"]
    );
    assert!(
        d.method_returns.is_empty(),
        "every stage-2 call site passes keep_predefined=false and is unchanged"
    );
}

// The `extract-dump` JSON is a byte-exact surface, so its KEY ORDER is
// significant in its own right.
pub(super) fn def_json_keys(d: &DefRecord) -> Vec<&'static str> {
    match def_to_json(d) {
        Json::Obj(fields) => fields.into_iter().map(|(k, _)| k).collect(),
        _ => panic!("def_to_json must produce an object"),
    }
}

#[test]
fn stage3_extension_methods_serialize_last_with_the_arity_range_and_this_args() {
    let e = extract_src(
            "namespace App.Ext { public static class W { public static Widget Copy(this Widget w) => w; public static string Trim(this string s, int n) => s; public static void Each(this List<Widget> l, params int[] xs) { } } }",
        );
    let d = find_def(&e, "App.Ext.W").expect("W def present");
    assert_eq!(
        def_json_keys(d),
        vec![
            "id",
            "name",
            "namespace",
            "kind",
            "line",
            "methods",
            "methodReturns",
            "extensionMethods"
        ],
        "extensionMethods lands AFTER the stage-2 additions, not among them"
    );
    let Json::Obj(fields) = def_to_json(d) else {
        panic!("object")
    };
    let (_, ext) = fields
        .into_iter()
        .find(|(k, _)| *k == "extensionMethods")
        .expect("extensionMethods present");
    let Json::Arr(entries) = ext else {
        panic!("extensionMethods is an array")
    };
    assert_eq!(entries.len(), 3);
    for entry in &entries[..2] {
        let Json::Obj(kv) = entry else {
            panic!("each entry is an object")
        };
        assert_eq!(
            kv.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            vec!["name", "thisType", "arityMin", "arityMax"],
            "entry key order is significant too"
        );
    }
    // The two arity halves are NUMBERS, not strings: a byte-diff would
    // catch a quoted one. arityMax is
    // SIGNED: -1 is the unbounded-`params` sentinel.
    let Json::Obj(kv) = &entries[1] else {
        panic!("object")
    };
    match kv.iter().find(|(k, _)| *k == "arityMin").map(|(_, v)| v) {
        Some(Json::Num(n)) => {
            assert_eq!(*n, 1, "Trim(this string s, int n) requires one argument")
        }
        _ => panic!("arityMin must serialize as a JSON number"),
    }
    match kv.iter().find(|(k, _)| *k == "arityMax").map(|(_, v)| v) {
        Some(Json::Int(n)) => assert_eq!(*n, 1),
        _ => panic!("arityMax must serialize as a JSON number"),
    }
    // The generic entry carries thisArgs, LAST, and its params tail makes
    // arityMax the -1 sentinel.
    let Json::Obj(kv) = &entries[2] else {
        panic!("object")
    };
    assert_eq!(
        kv.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
        vec!["name", "thisType", "arityMin", "arityMax", "thisArgs"],
        "thisArgs lands after arityMax, and only on a generic this-parameter"
    );
    assert!(
        matches!(
            kv.iter().find(|(k, _)| *k == "arityMax").map(|(_, v)| v),
            Some(Json::Int(-1))
        ),
        "a params tail serializes arityMax as the number -1"
    );
}

#[test]
fn stage3_bases_serializes_after_extension_methods_and_is_absent_when_empty() {
    // `Ns.BaseWidget<int>` carries a type-argument list, so this def now
    // ALSO gets a baseGenericArgs entry -- bases is no
    // longer the last key when a base is itself generic, which is the
    // point this fixture was chosen to cover: both keys' relative order
    // still holds (bases before baseGenericArgs, both before the absent
    // testMethods).
    let e = extract_src(
        "namespace App.Other { public class Widget : Ns.BaseWidget<int>, IWidget { } }",
    );
    let d = find_def(&e, "App.Other.Widget").expect("Widget def present");
    assert_eq!(
        def_json_keys(d),
        vec![
            "id",
            "name",
            "namespace",
            "kind",
            "line",
            "methods",
            "bases",
            "baseGenericArgs"
        ],
        "bases lands after methods, baseGenericArgs immediately after bases"
    );
    assert_eq!(
        d.bases,
        vec!["BaseWidget".to_string(), "IWidget".to_string()],
        "base IDENTIFIERS: generic arguments stripped, a qualified name cut to its last segment"
    );
    assert_eq!(
            d.base_generic_args,
            vec![("BaseWidget".to_string(), vec!["int".to_string()])],
            "BaseWidget<int> is non-generic Widget's closed base -- int is a predefined type, KEPT (this base pass keeps predefined types the same way the this-parameter thisArgs facts do); IWidget carries no type-argument list at all, so it contributes no entry"
        );

    let plain = extract_src("namespace App.Other { public class Bare { } }");
    let pd = find_def(&plain, "App.Other.Bare").expect("Bare def present");
    assert_eq!(
        def_json_keys(pd),
        vec!["id", "name", "namespace", "kind", "line", "methods"]
    );
}

#[test]
fn stage3_extension_methods_key_is_absent_when_the_type_declares_none() {
    let e = extract_src(
        "namespace App.Plain { public static class Utils { public static void Go(Widget w) { } } }",
    );
    let d = find_def(&e, "App.Plain.Utils").expect("Utils def present");
    assert_eq!(
        def_json_keys(d),
        vec!["id", "name", "namespace", "kind", "line", "methods"],
        "pre-stage-3 bytes preserved"
    );
}
