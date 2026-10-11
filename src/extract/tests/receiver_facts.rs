use super::def_member_facts::member_facts;
use super::preproc_duplicate_header::DUPLICATE_HEADER_HOST_WITH_NESTED_EMITTER_SRC;
use super::*;

// --- Receiver facts ---------------------------------------

#[test]
fn stage2b_facts_come_from_locals_var_new_params_fields_and_primary_ctor_params() {
    let e = extract_src(
        r#"
namespace App.Receivers;

public class Host(IClock clock)
{
  private readonly ILogger _log;

  public void Run(IRepo repo)
  {
    Helper h = new Helper();
    var svc = new Service();
    repo.Save();
    h.Help();
    svc.Go();
    _log.Info();
    clock.Now();
  }
}
"#,
    );
    assert_eq!(
        member_facts(&e),
        vec![
            ("Save", Some("IRepo")),
            ("Help", Some("Helper")),
            ("Go", Some("Service")),
            ("Info", Some("ILogger")),
            ("Now", Some("IClock")),
        ]
    );
}

#[test]
fn stage2b_receiver_type_is_set_only_on_refs_that_earned_a_fact() {
    let e = extract_src(
        r#"
namespace App.Shape;

public class Host
{
  public void Run(IRepo repo) { repo.Save(); }
}
"#,
    );
    let r = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member")
        .expect("member ref present");
    assert_eq!(r.name, "repo");
    assert_eq!(r.qualified, None);
    assert!(!r.generic);
    assert_eq!(r.receiver_type.as_deref(), Some("IRepo"));
}

#[test]
fn stage2b_no_fact_for_var_from_call_predefined_types_or_an_implicit_lambda_parameter() {
    let e = extract_src(
        r#"
namespace App.NoFacts;

public class Host
{
  public void Run(string text, System.Collections.Generic.Dictionary<string, Widget> items)
  {
    var computed = Compute();
    int count = 0;
    computed.Use();
    text.Trim();
    count.ToString();
    items.ForEach(x => x.Do());
  }
}
"#,
    );
    let facts: Vec<(String, Option<&str>)> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-member")
        .map(|r| {
            (
                format!("{}.{}", r.name, r.member.as_deref().unwrap_or("")),
                r.receiver_type.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        facts,
        vec![
            // var + a call initializer: resolve-time territory, never guessed here
            ("computed.Use".to_string(), None),
            // predefined types: no fact
            ("text.Trim".to_string(), None),
            ("count.ToString".to_string(), None),
            // an explicitly typed parameter DOES earn a fact even when its
            // declared type is qualified AND generic -- reduced to
            // "Dictionary" (a TWO-argument generic, deliberately, so this
            // window does NOT also qualify for the lambda-parameter
            // element-typing rule below -- that positive case has its own
            // dedicated stage4_first_single_parameter_lambda_... test)
            ("items.ForEach".to_string(), Some("Dictionary")),
            // implicit lambda parameter, first argument of a call whose
            // receiver is a TWO-argument generic: still no fact
            ("x.Do".to_string(), None),
        ]
    );
}

#[test]
fn stage2b_two_declarations_of_one_local_with_different_types_cancel_to_no_fact() {
    let e = extract_src(
        r#"
namespace App.Conflict;

public class Host
{
  public void Run(bool flag)
  {
    if (flag) { Widget x = new Widget(); x.Go(); }
    else { Gadget x = new Gadget(); x.Go(); }
  }
}
"#,
    );
    assert_eq!(
        member_facts(&e),
        vec![("Go", None), ("Go", None)],
        "the method-level flat table is conflicted for \"x\""
    );
}

#[test]
fn stage2b_same_local_name_declared_twice_with_the_same_type_still_yields_the_fact() {
    let e = extract_src(
        r#"
namespace App.NoConflict;

public class Host
{
  public void Run(bool flag)
  {
    if (flag) { Widget x = new Widget(); x.Go(); }
    else { Widget x = new Widget(); x.Go(); }
  }
}
"#,
    );
    assert_eq!(
        member_facts(&e),
        vec![("Go", Some("Widget")), ("Go", Some("Widget"))]
    );
}

#[test]
fn stage2b_a_parameter_shadows_a_same_named_field_of_a_different_type() {
    let e = extract_src(
        r#"
namespace App.Shadow;

public class Host
{
  private Widget handler;

  public void Run(Gadget handler)
  {
    handler.Go();
  }

  public void Untouched()
  {
    handler.Go();
  }
}
"#,
    );
    assert_eq!(
        member_facts(&e),
        vec![("Go", Some("Gadget")), ("Go", Some("Widget"))],
        "inside Run the parameter wins; the other method still sees the field"
    );
}

#[test]
fn stage2b_a_nested_type_never_inherits_the_enclosing_types_field_table() {
    let e = extract_src(
        r#"
namespace App.Nested;

public class Outer
{
  private Widget handler;

  public class Inner
  {
    public void Run() { handler.Go(); }
  }
}
"#,
    );
    assert_eq!(
        member_facts(&e),
        vec![("Go", None)],
        "a nested type cannot reach an outer instance field"
    );
}

#[test]
fn stage2b_a_flattened_chain_tail_never_inherits_the_heads_receiver_type() {
    // Stage-1 chain-tail regression class, extended for receiverType: a
    // tail must NOT inherit a fact it did not earn. Structurally
    // impossible here because the tail's qualifier is dotted.
    let e = extract_src(
        r#"
namespace App.Consumers;

public class Chain
{
  public void Run()
  {
    Widget w = new Widget();
    w.Inner.Tail();
  }
}
"#,
    );
    let head = e
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Inner"))
        .expect("head ref present");
    let tail = e
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Tail"))
        .expect("tail ref present");
    assert_eq!(
        head.receiver_type.as_deref(),
        Some("Widget"),
        "the head is the ref the local declaration vouches for"
    );
    assert_eq!(
        tail.qualified.as_deref(),
        Some("w.Inner"),
        "the tail's qualifier is the flattened chain window"
    );
    assert_eq!(
        tail.receiver_type, None,
        "the tail must NOT inherit the head's receiverType"
    );
}

#[test]
fn stage2b_a_generic_qualifier_never_carries_a_receiver_fact() {
    // A type-argument list is syntax no local, parameter or field can
    // carry, so the bare-only guard refuses the lookup outright even when
    // a same-named local happens to exist.
    let e = extract_src(
        r#"
namespace App.Generic;

public class Host
{
  public void Run()
  {
    Widget Cache = new Widget();
    Cache<int>.Go();
  }
}
"#,
    );
    let r = e
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Go"))
        .expect("member ref present");
    assert!(r.generic);
    assert_eq!(r.receiver_type, None);
}

#[test]
fn stage2b_a_local_declared_in_a_using_or_for_statement_is_a_fact() {
    let e = extract_src(
        r#"
namespace App.Statements;

public class Host
{
  public void Run()
  {
    using (Stream s = Open()) { s.Read(); }
    for (Counter c = Start(); ; ) { c.Tick(); }
  }
}
"#,
    );
    assert_eq!(
        member_facts(&e),
        vec![("Read", Some("Stream")), ("Tick", Some("Counter"))]
    );
}

#[test]
fn stage2b_an_explicitly_typed_foreach_variable_is_a_fact() {
    // foreach_statement carries its own `type`/`left` fields,
    // not a variable_declaration node, so it needs its own rule rather
    // than falling out of the ordinary declaration walk for free. An
    // explicitly typed loop variable is answered by that type node
    // exactly like any other declaration.
    let e = extract_src(
        r#"
namespace App.ForEach;

public class Host
{
  public void Run(System.Collections.Generic.List<Widget> items)
  {
    foreach (Widget w in items) { w.Go(); }
  }
}
"#,
    );
    let go = e
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Go"))
        .expect("member ref present");
    assert_eq!(go.receiver_type.as_deref(), Some("Widget"));
}

#[test]
fn stage2b_a_local_function_folds_into_the_enclosing_members_flat_table() {
    let e = extract_src(
        r#"
namespace App.LocalFn;

public class Host
{
  public void Run()
  {
    Widget w = new Widget();
    void Inner() { w.Go(); }
  }
}
"#,
    );
    let go = e
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Go"))
        .expect("member ref present");
    assert_eq!(
        go.receiver_type.as_deref(),
        Some("Widget"),
        "a local function sees the enclosing method's locals"
    );
}

#[test]
fn stage2b_an_explicitly_typed_declaration_never_reads_its_new_initializer() {
    let e = extract_src(
        r#"
namespace App.Explicit;

public class Host
{
  public void Run()
  {
    object o = new Widget();
    o.Go();
  }
}
"#,
    );
    let go = e
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Go"))
        .expect("member ref present");
    assert_eq!(
        go.receiver_type, None,
        "`object` is a predefined type: no fact, and Widget is never substituted for it"
    );
}

// --- The written qualifier alongside the simple receiver name ------------
//
// A receiver declared with a qualified type must keep that qualification:
// reducing `Outbox.OutboxSendEndpoint` to `OutboxSendEndpoint` leaves the
// resolver nothing to tell it apart from a same-named sibling, and the
// ladder's bare-name steps answer from a `using` the declaration never saw.

/// Every `uses-member` ref's (name, member, simple receiver type, written
/// qualifier), in extraction order.
fn receiver_facts_with_qualifier(
    e: &Extraction,
) -> Vec<(String, String, Option<String>, Option<String>)> {
    e.refs
        .iter()
        .filter(|r| r.kind == "uses-member")
        .map(|r| {
            (
                r.name.clone(),
                r.member.clone().unwrap_or_default(),
                r.receiver_type.clone(),
                r.receiver_type_qualified.clone(),
            )
        })
        .collect()
}

#[test]
fn qualified_receiver_type_survives_every_direct_declaration_path() {
    let e = extract_src(
        r#"
namespace App.Qualified;

public class Host
{
  private Widgets.Widget _field;

  public void Run(Widgets.Widget param)
  {
    Widgets.Widget local = null;
    var cast = (Widgets.Widget)param;
    var made = new Widgets.Widget();
    if (param is Widgets.Widget named) { named.Go(); }
    _field.Go();
    param.Go();
    local.Go();
    cast.Go();
    made.Go();
  }
}
"#,
    );
    let qualified = receiver_facts_with_qualifier(&e);
    for (name, member, simple, written) in &qualified {
        assert_eq!(simple.as_deref(), Some("Widget"), "{name}.{member}");
        assert_eq!(
            written.as_deref(),
            Some("Widgets.Widget"),
            "{name}.{member} lost the written path"
        );
    }
    assert_eq!(
        qualified
            .iter()
            .map(|(name, _, _, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["named", "_field", "param", "local", "cast", "made"]
    );
}

#[test]
fn qualified_receiver_type_covers_namespace_nested_and_alias_paths() {
    let e = extract_src(
        r#"
namespace App.Qualified;

public class Host
{
  public void Run(App.Widgets.Widget namespaced, Outer.Inner nested, global::App.Widgets.Widget aliased)
  {
    namespaced.Go();
    nested.Go();
    aliased.Go();
  }
}
"#,
    );
    let facts: Vec<(String, Option<String>, Option<String>)> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-member")
        .map(|r| {
            (
                r.name.clone(),
                r.receiver_type.clone(),
                r.receiver_type_qualified.clone(),
            )
        })
        .collect();
    assert_eq!(
        facts,
        vec![
            (
                "namespaced".to_string(),
                Some("Widget".to_string()),
                Some("App.Widgets.Widget".to_string())
            ),
            (
                "nested".to_string(),
                Some("Inner".to_string()),
                Some("Outer.Inner".to_string())
            ),
            (
                "aliased".to_string(),
                Some("Widget".to_string()),
                Some("global::App.Widgets.Widget".to_string())
            ),
        ]
    );
}

#[test]
fn qualified_receiver_type_keeps_wrappers_and_generic_arguments() {
    // The wrappers are unwrapped for the simple name -- `base_type_identifier`
    // has always looked through them -- but the path is what the declaration
    // WROTE, so a nullable or array wrapper leaves the text alone and a
    // generic argument list stays in it for the ladder's own normalization.
    let e = extract_src(
        r#"
namespace App.Qualified;

public class Host
{
  public void Run(Widgets.Widget? optional, Widgets.Widget[] many, Widgets.Box<int> generic)
  {
    optional.Go();
    many.Go();
    generic.Go();
  }
}
"#,
    );
    let facts: Vec<(String, Option<String>, Option<String>)> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-member")
        .map(|r| {
            (
                r.name.clone(),
                r.receiver_type.clone(),
                r.receiver_type_qualified.clone(),
            )
        })
        .collect();
    assert_eq!(
        facts,
        vec![
            (
                "optional".to_string(),
                Some("Widget".to_string()),
                Some("Widgets.Widget".to_string())
            ),
            (
                "many".to_string(),
                Some("Widget".to_string()),
                Some("Widgets.Widget".to_string())
            ),
            (
                "generic".to_string(),
                Some("Box".to_string()),
                Some("Widgets.Box<int>".to_string())
            ),
        ]
    );
}

#[test]
fn two_same_tail_qualified_declarations_conflict_like_different_names() {
    let e = extract_src(
        r#"
namespace App.Qualified;

public class Host
{
  public void Run(bool flag)
  {
    if (flag) { Alpha.Widget x = null; x.Go(); }
    else { Beta.Widget x = null; x.Go(); }
  }
}
"#,
    );
    assert_eq!(
        member_facts(&e),
        vec![("Go", None), ("Go", None)],
        "the qualifier is part of the fact, so the two declarations conflict"
    );
}

#[test]
fn an_unqualified_declaration_keeps_the_absent_qualifier() {
    let e = extract_src(
        r#"
namespace App.Qualified;

public class Host
{
  public void Run(Widget param)
  {
    param.Go();
  }
}
"#,
    );
    let r = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member")
        .expect("member ref present");
    assert_eq!(r.receiver_type.as_deref(), Some("Widget"));
    assert_eq!(r.receiver_type_qualified, None);
}

#[test]
fn stage2b_the_surviving_header_arm_supplies_receiver_facts_at_both_nesting_levels() {
    // The duplicate-header preproc shape (see
    // DUPLICATE_HEADER_HOST_WITH_NESTED_EMITTER_SRC), at both nesting
    // levels. `WIDGET_V2` is undefined, so in each of the two methods
    // only the `#else` arm's `IReadOnlyList<Widget> items` parameter
    // reaches the parser -- each `items` ref vouches through its own
    // method's parameter, and `sink` vouches through the inner
    // method's own (unconditional) parameter.
    let e = extract_src(DUPLICATE_HEADER_HOST_WITH_NESTED_EMITTER_SRC);
    let facts: Vec<(&str, Option<&str>)> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-member")
        .map(|r| (r.name.as_str(), r.receiver_type.as_deref()))
        .collect();
    assert_eq!(
        facts,
        vec![
            ("items", Some("IReadOnlyList")),
            ("items", Some("IReadOnlyList")),
            ("sink", Some("Sink")),
        ]
    );
}

#[test]
fn the_dump_carries_the_qualified_receiver_keys_and_omits_them_when_absent() {
    let e = extract_src(
        r#"
namespace App.Qualified;

public class Host
{
  public Widgets.Widget Dial { get; }

  public void Run(Widgets.Widget param)
  {
    param.Go();
  }
}
"#,
    );
    let json = extraction_to_json(&e);
    assert!(
        json.contains(r#""receiverTypeQualified": "Widgets.Widget""#),
        "the ref keeps the written path: {json}"
    );
    assert!(
        json.contains(r#""qualifiedType": "Widgets.Widget""#),
        "and so does the declared type fact the resolver reads: {json}"
    );
    assert_eq!(
        json.matches("receiverTypeQualified").count(),
        1,
        "the unqualified receiver adds no key at all: {json}"
    );
    assert_eq!(
        json.matches("qualifiedType").count(),
        1,
        "and neither does an unqualified declaration: {json}"
    );
}
