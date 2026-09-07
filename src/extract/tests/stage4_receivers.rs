use super::*;

// --- Stage 4: this/base/conditional receivers, await/cast/pattern facts ----

#[test]
fn stage4_this_qualifier_yields_a_uses_member_ref_typed_by_the_enclosing_type() {
    let e = extract_src(
        r#"
namespace Fixtures.Recall;
public class Order
{
    public void Describe()
    {
        this.Validate();
    }
}
"#,
    );
    let r = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Validate"))
        .expect("this-qualified ref present");
    assert_eq!(r.name, "Order");
    assert_eq!(r.qualified, None);
    assert_eq!(r.receiver_type.as_deref(), Some("Order"));
    assert_eq!(r.receiver_args, None, "Order is not generic");
    assert!(!r.receiver_base, "plain this. never starts at the bases");
    assert_eq!(r.outer_types, vec!["Order".to_string()]);
    assert!(!r.generic);

    // A generic enclosing type's OWN type parameters vouch for
    // receiver_args, one "*" wildcard per parameter.
    let eg = extract_src(
        r#"
namespace Fixtures.Recall;
public class Box<T>
{
    public void Use()
    {
        this.Reset();
    }
}
"#,
    );
    let rg = eg
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Reset"))
        .expect("this-qualified ref present in the generic type");
    assert_eq!(rg.receiver_type.as_deref(), Some("Box"), "no arity suffix");
    assert_eq!(rg.receiver_args, Some(vec!["*".to_string()]));

    // A `this.` site outside any type -- a top-level statement -- emits
    // no ref at all: an empty type_stack denotes no enclosing type.
    let top = extract_src("this.Validate();");
    assert!(top
        .refs
        .iter()
        .all(|r| !(r.kind == "uses-member" && r.member.as_deref() == Some("Validate"))));
}

#[test]
fn stage4_base_qualifier_yields_a_ref_that_starts_lookup_at_the_bases() {
    let e = extract_src(
        r#"
namespace Fixtures.Recall;
public class Order : Entity
{
    public void Describe()
    {
        base.Touch();
        this.Touch();
    }
}
"#,
    );
    let base_ref = e
        .refs
        .iter()
        .find(|r| {
            r.kind == "uses-member" && r.member.as_deref() == Some("Touch") && r.receiver_base
        })
        .expect("base-qualified ref present");
    assert_eq!(base_ref.name, "Order");
    assert_eq!(base_ref.receiver_type.as_deref(), Some("Order"));
    assert!(base_ref.receiver_base);

    let this_ref = e
        .refs
        .iter()
        .find(|r| {
            r.kind == "uses-member" && r.member.as_deref() == Some("Touch") && !r.receiver_base
        })
        .expect("plain this-qualified ref present");
    assert!(
        !this_ref.receiver_base,
        "plain this. carries receiver_base == false"
    );

    // `receiverBase` is OMITTED from the serialized fragment JSON when
    // false (the file's `is_false` idiom, see graph.rs), never written
    // as `"receiverBase":false`.
    let fragment = crate::graph::fragment_from_extraction(&e);
    let touch_refs: Vec<&crate::graph::FragRef> = fragment
        .refs
        .iter()
        .filter(|r| r.member.as_deref() == Some("Touch"))
        .collect();
    assert_eq!(touch_refs.len(), 2);
    let jsons: Vec<String> = touch_refs
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect();
    assert!(
        jsons.iter().any(|j| j.contains("\"receiverBase\":true")),
        "the base. window carries receiverBase: {jsons:?}"
    );
    assert!(
        jsons.iter().any(|j| !j.contains("receiverBase")),
        "the plain this. window omits receiverBase entirely: {jsons:?}"
    );
}

#[test]
fn stage4_receiver_local_is_recorded_for_a_shadowing_name_and_omitted_otherwise() {
    let e = extract_src(
        r#"
namespace Fixtures.Recall;
public class Widget
{
    public void Poke()
    {
        var order = Unknown();
        order.Spin();
        other.Spin();
    }
}
"#,
    );
    // `Unknown()` is a BARE (undotted) call -- a shape `invocation_call`
    // never matches -- so `order` settles as an ordinary
    // taken-but-unknown member-table entry: no `Fact` vouches for its
    // type, but the name IS in scope.
    let order_ref = e
        .refs
        .iter()
        .find(|r| {
            r.kind == "uses-member" && r.name == "order" && r.member.as_deref() == Some("Spin")
        })
        .expect("order.Spin() ref present");
    assert!(
        order_ref.receiver_local,
        "order is taken by an in-file member-table entry (untyped), so receiver_local is true"
    );
    assert!(order_ref.receiver_type.is_none());

    let other_ref = e
        .refs
        .iter()
        .find(|r| {
            r.kind == "uses-member" && r.name == "other" && r.member.as_deref() == Some("Spin")
        })
        .expect("other.Spin() ref present");
    assert!(
        !other_ref.receiver_local,
        "other has no in-file fact of any kind -- not even a taken-but-unknown entry -- so \
             receiver_local is false"
    );

    // `receiverLocal` is OMITTED from the serialized fragment JSON when
    // false (the file's `is_false` idiom, see graph.rs), never written
    // as `"receiverLocal":false`.
    let fragment = crate::graph::fragment_from_extraction(&e);
    let spin_refs: Vec<&crate::graph::FragRef> = fragment
        .refs
        .iter()
        .filter(|r| r.member.as_deref() == Some("Spin"))
        .collect();
    assert_eq!(spin_refs.len(), 2);
    let jsons: Vec<String> = spin_refs
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect();
    assert!(
        jsons.iter().any(|j| j.contains("\"receiverLocal\":true")),
        "order.Spin() carries receiverLocal: {jsons:?}"
    );
    assert!(
        jsons.iter().any(|j| !j.contains("receiverLocal")),
        "other.Spin() omits receiverLocal entirely: {jsons:?}"
    );
}

#[test]
fn stage4_conditional_access_yields_the_same_ref_as_plain_access() {
    let plain = extract_src(
        r#"
namespace Fixtures.Recall;
public class Client
{
    private Http _http;
    public void Close()
    {
        _http.Dispose();
    }
}
"#,
    );
    let cond = extract_src(
        r#"
namespace Fixtures.Recall;
public class Client
{
    private Http _http;
    public void Close()
    {
        _http?.Dispose();
    }
}
"#,
    );
    let p = plain
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Dispose"))
        .expect("plain access ref present");
    let c = cond
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Dispose"))
        .expect("conditional access ref present");
    assert_eq!(c.kind, p.kind);
    assert_eq!(c.name, p.name);
    assert_eq!(c.qualified, p.qualified);
    assert_eq!(c.member, p.member);
    assert_eq!(c.namespace, p.namespace);
    assert_eq!(c.type_arg_count, p.type_arg_count);
    assert_eq!(c.generic, p.generic);
    assert_eq!(c.receiver_type, p.receiver_type);
    assert_eq!(c.receiver_type.as_deref(), Some("Http"));
    assert_eq!(c.arg_count, p.arg_count);
    assert_eq!(c.receiver_args, p.receiver_args);
    assert_eq!(c.outer_types, p.outer_types);
    assert_eq!(c.args, p.args);
    assert_eq!(c.receiver_property_owner, p.receiver_property_owner);
    assert_eq!(c.receiver_call_owner, p.receiver_call_owner);
    assert_eq!(c.receiver_call_member, p.receiver_call_member);
    assert_eq!(c.receiver_base, p.receiver_base);
    assert_eq!(c.receiver_awaited, p.receiver_awaited);
    // line deliberately not compared -- the two sources place the
    // access on the same source line here, but the fields above are
    // the actual guarantee.
}

#[test]
fn stage4_await_wrapped_invocation_and_creation_still_yield_a_fact() {
    let call = extract_src(
        r#"
namespace Fixtures.Recall;
public class Worker
{
    public async Task Run()
    {
        var order = await Repo.LoadAsync();
        order.Validate();
    }
}
"#,
    );
    let validate = call
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Validate"))
        .expect("order.Validate() ref present");
    assert_eq!(
        validate.receiver_call_owner.as_deref(),
        Some("Repo"),
        "the awaited call fact still records its callee's qualifier"
    );
    assert_eq!(validate.receiver_call_member.as_deref(), Some("LoadAsync"));
    assert_eq!(validate.receiver_type, None);
    assert!(
        validate.receiver_awaited,
        "the call fact came from an AWAITED invocation"
    );

    let creation = extract_src(
        r#"
namespace Fixtures.Recall;
public class Worker
{
    public async Task Run()
    {
        var widget = await new Widget();
        widget.Render();
    }
}
"#,
    );
    let render = creation
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Render"))
        .expect("widget.Render() ref present");
    assert_eq!(render.receiver_type.as_deref(), Some("Widget"));
    assert!(
        !render.receiver_awaited,
        "a constructed-type receiver fact never carries a call, so it never carries awaited \
             either"
    );
}

#[test]
fn stage4_method_return_args_records_one_level_of_generic_args_for_task_wrapped_returns() {
    let e = extract_src(
        r#"
namespace Fixtures.Recall;
public class Repo
{
    public Task<Order> LoadAsync() => null;
    public ValueTask<Order> LoadFastAsync() => null;
    public Task<Task<Order>> LoadNestedAsync() => null;
    public Order LoadSync() => null;
    public void Nothing() { }
}
"#,
    );
    let d = find_def(&e, "Fixtures.Recall.Repo").expect("Repo def present");
    assert_eq!(
        d.method_returns,
        vec![
            ("LoadAsync".to_string(), "Task".to_string()),
            ("LoadFastAsync".to_string(), "ValueTask".to_string()),
            ("LoadNestedAsync".to_string(), "Task".to_string()),
            ("LoadSync".to_string(), "Order".to_string()),
        ],
        "unchanged: the bare return-type identifier, generic args stripped, exactly as \
             raw_method_returns has always recorded it"
    );
    let args: Vec<_> = d
        .method_return_args
        .iter()
        .map(|(n, a)| (n.as_str(), a.as_slice()))
        .collect();
    assert_eq!(
        args,
        vec![
            ("LoadAsync", &["Order".to_string()][..]),
            ("LoadFastAsync", &["Order".to_string()][..]),
            ("LoadNestedAsync", &["Task".to_string()][..]),
        ],
        "one level of generic-arg descriptors, present only for a name method_returns ALSO \
             recorded an entry for -- LoadSync (non-generic) and Nothing (no method_returns entry \
             at all) contribute nothing, and LoadNestedAsync's own descriptor is the INNER Task's \
             bare name, never its own further-nested Order"
    );
}

#[test]
fn stage4_field_declarations_record_field_types() {
    let e = extract_src(
        r#"
namespace App.FieldTypes;

public class Widget
{
  private Settings _config, _fallback;
  private string _label;
  private Box<Gadget> _slots;
}
"#,
    );
    let d = find_def(&e, "App.FieldTypes.Widget").expect("Widget def present");
    assert_eq!(d.fields, vec!["_config", "_fallback", "_label", "_slots"]);
    let recorded: Vec<_> = d
        .field_types
        .iter()
        .map(|(n, f)| (n.as_str(), f.type_name.as_str(), f.args.as_ref()))
        .collect();
    // A predefined type vouches for nothing, exactly as it does for a
    // property, local or method return, so `_label` has no entry -- the
    // map is parallel to `fields` but not equal in length. `_config` and
    // `_fallback` share ONE field_declaration's type node ("private
    // Settings _config, _fallback;"), so they share the SAME fact.
    assert_eq!(
        recorded,
        vec![
            ("_config", "Settings", None),
            ("_fallback", "Settings", None),
            ("_slots", "Box", Some(&vec!["Gadget".to_string()])),
        ]
    );
}

#[test]
fn stage4_non_public_members_are_recorded_in_their_own_lists() {
    let e = extract_src(
        r#"
namespace App.Visibility;

public interface IWidget
{
    void Contract();
}

public class Widget : IWidget
{
    public void PublicMethod() { }
    protected void ProtectedMethod() { }
    internal void InternalMethod() { }
    private void PrivateMethod() { }
    void DefaultMethod() { }
    public void Contract() { }
}
"#,
    );
    let d = find_def(&e, "App.Visibility.Widget").expect("Widget def present");
    assert_eq!(
        d.methods,
        vec!["PublicMethod", "Contract"],
        "the public list is unchanged by this unit"
    );
    assert_eq!(
        d.non_public_methods,
        vec![
            "ProtectedMethod",
            "InternalMethod",
            "PrivateMethod",
            "DefaultMethod",
        ],
        "the exact complement, same source order, same method_declaration nodes -- every \
             method declaration lands in exactly one of the two lists"
    );

    let iface = find_def(&e, "App.Visibility.IWidget").expect("IWidget def present");
    assert_eq!(iface.methods, vec!["Contract"]);
    assert!(
        iface.non_public_methods.is_empty(),
        "every interface method already counts as public -- is_recorded_method's own \
             kind == \"interface\" short-circuit -- so this list is always empty for an interface, \
             by construction rather than by a second check"
    );
}

#[test]
fn stage4_method_arities_are_recorded_per_overload_with_params_unbounded() {
    let e = extract_src(
        r#"
namespace App.Arity;

public class Mailer
{
    public void Touch() { }
    public void Send(string to) { }
    public void Send(string to, string cc = null) { }
    public void Spray(params string[] recipients) { }
    private void Log(string message) { }
}
"#,
    );
    let d = find_def(&e, "App.Arity.Mailer").expect("Mailer def present");
    let arities: Vec<(&str, &[(usize, i64)])> = d
        .method_arities
        .iter()
        .map(|(n, r)| (n.as_str(), r.as_slice()))
        .collect();
    assert_eq!(
        arities,
        vec![
            ("Touch", &[(0, 0)][..]),
            (
                "Send",
                // Two overloads sharing the name -- BOTH ranges recorded,
                // in declaration order: the required-only shape first,
                // the optional-parameter shape second. Neither discards
                // the other -- the resolver needs the OR of every
                // overload.
                &[(1, 1), (1, 2)][..]
            ),
            (
                "Spray",
                // A trailing `params` array is optional AND unbounded:
                // nothing forces it, nothing caps it -- the same -1
                // sentinel `ExtensionMethod::arity_max` already uses.
                &[(0, -1)][..]
            ),
            (
                "Log",
                // Non-public methods get an entry too: unlike `methods`,
                // `method_arities` is not filtered by accessibility --
                // the resolver's arity gate applies equally to a
                // `base.`/`this.` lookup against `non_public_methods`.
                &[(1, 1)][..]
            ),
        ]
    );
}

#[test]
fn stage4_cast_pattern_and_out_designations_yield_type_facts() {
    let cast = extract_src(
        r#"
namespace Fixtures.Recall;
public class Probe
{
    public void F(object e)
    {
        var x = (Widget)e;
        x.Render();
    }
}
"#,
    );
    let render = cast
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Render"))
        .expect("x.Render() ref present");
    assert_eq!(render.receiver_type.as_deref(), Some("Widget"));

    let pattern = extract_src(
        r#"
namespace Fixtures.Recall;
public class Probe
{
    public void F(object e)
    {
        if (e is Widget t)
        {
            t.Render();
        }
    }
}
"#,
    );
    let render = pattern
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Render"))
        .expect("t.Render() ref present");
    assert_eq!(render.receiver_type.as_deref(), Some("Widget"));

    let out_typed = extract_src(
        r#"
namespace Fixtures.Recall;
public class Probe
{
    public void F()
    {
        TryGet(out Widget x);
        x.Render();
    }
}
"#,
    );
    let render = out_typed
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Render"))
        .expect("x.Render() ref present");
    assert_eq!(render.receiver_type.as_deref(), Some("Widget"));

    // `out var x` stays fact-less.
    let out_var = extract_src(
        r#"
namespace Fixtures.Recall;
public class Probe
{
    public void F()
    {
        TryGet(out var x);
        x.Render();
    }
}
"#,
    );
    let render = out_var
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Render"))
        .expect("x.Render() ref present");
    assert_eq!(render.receiver_type, None, "out var x earns no fact");
}

#[test]
fn stage4_catch_declaration_designation_yields_a_type_fact() {
    let e = extract_src(
        r#"
namespace Fixtures.Recall;
public class Probe
{
    public void F()
    {
        try { Work(); }
        catch (WidgetException e) { e.Ship(); }
    }
}
"#,
    );
    let ship = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Ship"))
        .expect("e.Ship() ref present");
    assert_eq!(
        ship.receiver_type.as_deref(),
        Some("WidgetException"),
        "a caught exception is a declaration like any other -- the handler names its type"
    );
    assert!(
        ship.receiver_local,
        "and the member's own fact table claims the name, so it shadows a same-named field"
    );
}

#[test]
fn stage4_range_variables_and_untyped_lambda_parameters_take_the_name_without_a_type() {
    // Every name below is DECLARED by the syntax that introduces it, so
    // each has to claim a slot in the member's own fact table even
    // though nothing here says what its type is. `receiver_local` is
    // what the resolver reads to keep its field fallback off them.
    let queried = extract_src(
        r#"
namespace Fixtures.Recall;
public class Probe
{
    public void F()
    {
        var picked = from d in Items
                     join o in Others on d equals o into g
                     let n = Items
                     select d.Ship();
        var more = from x in Items select x into k select k.Ship();
        g.Ship();
        n.Ship();
        o.Ship();
    }
}
"#,
    );
    for name in ["d", "o", "g", "n", "k"] {
        let r = queried
            .refs
            .iter()
            .find(|r| {
                r.kind == "uses-member" && r.name == name && r.member.as_deref() == Some("Ship")
            })
            .unwrap_or_else(|| panic!("{name}.Ship() ref present"));
        assert!(
            r.receiver_local,
            "{name} is a query range variable, so the member's fact table claims it"
        );
        assert_eq!(
            r.receiver_type, None,
            "{name} is claimed without a type -- the element type of the source sequence is \
                 not something this extractor can compute"
        );
    }

    let lambdas = extract_src(
        r#"
namespace Fixtures.Recall;
public class Probe
{
    public void F()
    {
        Wrap(1, q => q.Ship());
        var f = w => w.Ship();
        Items.Select((a, b) => a.Ship());
        Items.Select((Order z) => z.Ship());
    }
}
"#,
    );
    for name in ["q", "w", "a"] {
        let r = lambdas
            .refs
            .iter()
            .find(|r| {
                r.kind == "uses-member" && r.name == name && r.member.as_deref() == Some("Ship")
            })
            .unwrap_or_else(|| panic!("{name}.Ship() ref present"));
        assert!(
            r.receiver_local,
            "{name} is a lambda parameter the element rule declines -- a non-first argument, a \
                 lambda that is no argument at all, a multi-parameter list -- and it is still a \
                 declaration"
        );
        assert_eq!(r.receiver_type, None, "with no type the rule could give it");
    }
    let typed = lambdas
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.name == "z")
        .expect("z.Ship() ref present");
    assert_eq!(
        typed.receiver_type.as_deref(),
        Some("Order"),
        "an explicitly typed lambda parameter keeps the fact its own declaration gives"
    );
}
