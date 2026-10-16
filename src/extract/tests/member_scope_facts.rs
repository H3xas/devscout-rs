use super::*;

fn runner(fields: &str, params: &str, body: &str) -> String {
    format!(
        "namespace App {{ public class Runner {{ {fields} \
         public void Run(Svc s, object o, (Gadget, Gadget) pair{params}) {{ {body} }} }} }}"
    )
}

fn calls<'a>(e: &'a Extraction, member: &'a str) -> impl Iterator<Item = &'a RefRecord> {
    e.refs
        .iter()
        .filter(move |r| r.kind == "uses-member" && r.member.as_deref() == Some(member))
}

// The first argument's type at every `Use` call, in source order.
fn use_args(source: &str) -> Vec<Option<String>> {
    let e = extract_src(source);
    calls(&e, "Use")
        .map(
            |r| match r.arg_types.as_ref().and_then(|a| a.first().cloned()) {
                Some(Some(ArgTypeRecord::Known(t))) => Some(t),
                Some(Some(ArgTypeRecord::Member { owner, member, .. })) => {
                    Some(format!("{owner}.{member}"))
                }
                _ => None,
            },
        )
        .collect()
}

// What every `Touch` call's receiver is typed as, and whether a member-table
// entry claims the receiver's name.
fn touch_receivers(source: &str) -> Vec<(Option<String>, bool)> {
    let e = extract_src(source);
    calls(&e, "Touch")
        .map(|r| {
            let typed = r
                .receiver_type
                .clone()
                .or_else(|| {
                    r.receiver_call_owner.as_ref().map(|o| {
                        format!("{o}.{}()", r.receiver_call_member.as_deref().unwrap_or(""))
                    })
                })
                .or_else(|| {
                    r.receiver_lambda
                        .as_ref()
                        .map(|slot| format!("slot {}.{}", slot.owner, slot.member))
                });
            (typed, r.receiver_local)
        })
        .collect()
}

#[test]
fn member_scope_ac_1_every_deconstruction_form_is_taken_but_unknown() {
    let fields = "Widget a; Widget c; Widget k; Widget x; Widget q; Widget m; Widget d;";
    let bodies = [
        "var (a, b) = pair; s.Use(a); a.Touch();",
        "var (a, (b, c)) = (pair.Item1, pair); s.Use(c); c.Touch();",
        "foreach (var (k, v) in pairs) { s.Use(k); k.Touch(); }",
        "if (o is var (x, y)) { s.Use(x); x.Touch(); }",
        "switch (o) { case var (q, r): s.Use(q); q.Touch(); break; }",
        "var z = o switch { var (m, n) => Pick(s.Use(m), m.Touch()), _ => 0 };",
        "(var d, var e) = pair; s.Use(d); d.Touch();",
    ];
    for body in bodies {
        let source = runner(fields, ", (Gadget, Gadget)[] pairs", body);
        assert_eq!(use_args(&source), vec![None], "{body}");
        assert_eq!(touch_receivers(&source), vec![(None, true)], "{body}");
    }
}

#[test]
fn member_scope_ac_1_explicitly_typed_elements_keep_their_types() {
    let source = runner(
        "Widget a; Widget w;",
        "",
        "(int a, Gadget w) = (1, pair.Item1); s.Use(a); w.Touch();",
    );
    assert_eq!(use_args(&source), vec![Some("int".to_string())]);
    assert_eq!(
        touch_receivers(&source),
        vec![(Some("Gadget".to_string()), true)]
    );
}

#[test]
fn member_scope_ac_2_a_use_outside_every_declaring_construct_withholds() {
    let bodies = [
        "s.Use(v); v.Touch(); { Gadget v = null; }",
        "{ Gadget v = null; } { s.Use(v); v.Touch(); }",
        "System.Action<Gadget> f = (Gadget v) => { }; s.Use(v); v.Touch();",
        "gs.ForEach(v => { }); s.Use(v); v.Touch();",
        "System.Action<Gadget> f = v => { }; s.Use(v); v.Touch();",
        "System.Action<Gadget> f = delegate (Gadget v) { }; s.Use(v); v.Touch();",
        "void Local(Gadget v) { } s.Use(v); v.Touch();",
        "foreach (Gadget v in gs) { } s.Use(v); v.Touch();",
        "try { } catch (GadgetException v) { } s.Use(v); v.Touch();",
        "var q = from v in gs select v; s.Use(v); v.Touch();",
        "{ if (o is Gadget v) { } } s.Use(v); v.Touch();",
        "{ Make(out Gadget v); } s.Use(v); v.Touch();",
        "switch (o) { case Gadget v: break; } s.Use(v); v.Touch();",
    ];
    for body in bodies {
        let source = runner(
            "Widget v;",
            ", System.Collections.Generic.List<Gadget> gs",
            body,
        );
        assert_eq!(use_args(&source), vec![None], "{body}");
        assert_eq!(touch_receivers(&source), vec![(None, true)], "{body}");
    }
}

#[test]
fn member_scope_ac_2_hops_read_from_an_out_of_scope_name_withhold() {
    let params = ", System.Collections.Generic.List<Gadget> gs";
    let bodies = [
        "{ Repo r = null; } var x = r.Load(); x.Touch();",
        "{ System.Collections.Generic.List<Gadget> os = null; } foreach (var x in os) { x.Touch(); }",
        "{ System.Collections.Generic.List<Gadget> os = null; } os.Where(x => x.Touch());",
        "{ Runner q = null; } q.Each(x => x.Touch());",
    ];
    for body in bodies {
        let source = runner("Widget r; Widget os; Widget q;", params, body);
        assert_eq!(touch_receivers(&source), vec![(None, true)], "{body}");
    }
}

#[test]
fn member_scope_ac_3_uses_inside_a_declaring_construct_keep_their_facts() {
    let gadget = || (Some("Gadget".to_string()), true);
    let cases = [
        "{ Gadget v = null; s.Use(v); v.Touch(); }",
        "Gadget v = null; s.Use(v); v.Touch();",
        "System.Action<Gadget> f = (Gadget v) => { s.Use(v); v.Touch(); };",
        "System.Action<Gadget> f = delegate (Gadget v) { s.Use(v); v.Touch(); };",
        "void Local(Gadget v) { s.Use(v); v.Touch(); }",
        "foreach (Gadget v in gs) { s.Use(v); v.Touch(); }",
        "try { } catch (Gadget v) { s.Use(v); v.Touch(); }",
        "{ if (o is Gadget v) { s.Use(v); v.Touch(); } }",
        "if (o is Gadget v) { } s.Use(v); v.Touch();",
        "{ Make(out Gadget v); s.Use(v); v.Touch(); }",
        "switch (o) { case Gadget v: s.Use(v); v.Touch(); break; }",
        "switch (o) { case 1: Gadget v = null; break; default: s.Use(v); v.Touch(); break; }",
        "for (Gadget v = null; ; ) { s.Use(v); v.Touch(); }",
        "using (Gadget v = null) { s.Use(v); v.Touch(); }",
    ];
    for body in cases {
        let source = runner(
            "Widget v;",
            ", System.Collections.Generic.List<Gadget> gs",
            body,
        );
        assert_eq!(
            use_args(&source),
            vec![Some("Gadget".to_string())],
            "{body}"
        );
        let receivers = touch_receivers(&source);
        assert_eq!(receivers, vec![gadget()], "{body}");
    }
}

#[test]
fn member_scope_ac_3_member_parameters_and_out_designations_reach_every_later_use() {
    let source = runner(
        "Widget v;",
        ", Gadget v",
        "s.Use(v); { s.Use(v); } System.Action a = () => s.Use(v); void L() { s.Use(v); }",
    );
    assert_eq!(use_args(&source), vec![Some("Gadget".to_string()); 4]);

    let typed = runner(
        "string n;",
        ", string t",
        "if (!int.TryParse(t, out int n)) return; s.Use(n);",
    );
    assert_eq!(use_args(&typed), vec![Some("int".to_string())]);

    let implicit = runner(
        "string n;",
        ", string t",
        "if (!int.TryParse(t, out var n)) return; s.Use(n); n.Touch();",
    );
    assert_eq!(use_args(&implicit), vec![None]);
    assert_eq!(touch_receivers(&implicit), vec![(None, true)]);
}

#[test]
fn member_scope_ac_3_derived_locals_keep_their_receiver_facts_inside_their_construct() {
    let params = ", Repo repo, System.Collections.Generic.List<Gadget> gs, Runner q";
    let cases = [
        ("{ var x = repo.Load(); x.Touch(); }", "Repo.Load()"),
        ("foreach (var x in gs) { x.Touch(); }", "Gadget"),
        ("gs.Where(x => x.Touch());", "Gadget"),
        ("q.Each(x => x.Touch());", "slot Runner.Each"),
    ];
    for (body, want) in cases {
        let source = runner("Widget x;", params, body);
        assert_eq!(
            touch_receivers(&source),
            vec![(Some(want.to_string()), true)],
            "{body}"
        );
    }
}

#[test]
fn member_scope_ac_2_a_declaration_with_no_placeable_construct_is_unknown_everywhere() {
    use super::super::member_scope::MemberTable;
    use super::super::receivers::type_fact;
    use super::super::text::new_parser;

    let source = "delegate void D(Gadget v);";
    let tree = new_parser().parse(source, None).expect("parse");
    let mut cursor = tree.root_node().walk();
    let mut stack = vec![tree.root_node()];
    let mut parameter = None;
    while let Some(node) = stack.pop() {
        if node.kind() == "parameter" {
            parameter = Some(node);
            break;
        }
        stack.extend(node.named_children(&mut cursor));
    }
    let parameter = parameter.expect("a parameter node");
    let fact = type_fact(
        parameter.child_by_field_name("type"),
        source.as_bytes(),
        &std::collections::HashSet::new(),
    );
    assert!(fact.is_some());

    let mut table = MemberTable::new();
    table.declare(Some("v".to_string()), fact, parameter);
    assert!(table.contains("v"));
    assert!(matches!(
        table.lookup("v", parameter.start_byte()),
        Some(None)
    ));
    assert!(matches!(table.lookup("v", 0), Some(None)));
}
