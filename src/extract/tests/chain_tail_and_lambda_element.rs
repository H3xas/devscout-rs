use super::*;

// --- Unit C: chain-tail receivers and lambda-parameter element typing --

#[test]
fn stage4_call_chain_tail_carries_the_inner_call_as_its_receiver() {
    let e = extract_src(
        r#"
namespace App.ChainTail;

public class Host
{
  private Widget a;

  public void Run()
  {
    var y = a.B().C();
    var z = Repo.Load().Validate();
  }
}
"#,
    );
    // `a`: an IN-FILE fact (the field `a: Widget`). `Repo`: nothing in
    // scope claims the name, so its own bare text is the static-type
    // candidate -- the "Static type qualifier ... counts as typed" case.
    let c = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("C"))
        .expect("a.B().C() earns a ref for its own window");
    assert_eq!(
        c.receiver_type, None,
        "a call-shaped receiver, never a resolved type"
    );
    assert_eq!(c.receiver_call_owner.as_deref(), Some("Widget"));
    assert_eq!(c.receiver_call_member.as_deref(), Some("B"));
    assert!(!c.generic);
    assert!(!c.receiver_base);
    assert_eq!(c.arg_count, Some(0), "C() itself takes zero arguments");

    let validate = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Validate"))
        .expect("Repo.Load().Validate() earns a ref for its own window");
    assert_eq!(validate.receiver_type, None);
    assert_eq!(validate.receiver_call_owner.as_deref(), Some("Repo"));
    assert_eq!(validate.receiver_call_member.as_deref(), Some("Load"));
}

#[test]
fn stage4_base_qualified_chain_head_marks_the_tail_ref_as_base() {
    let e = extract_src(
        r#"
namespace App.ChainTail;

public class Use : BaseC
{
  public void Run()
  {
    base.Make().Validate();
    this.Make().Validate();
  }
}
"#,
    );
    let tails: Vec<(Option<&str>, bool)> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Validate"))
        .map(|r| (r.qualified.as_deref(), r.receiver_base))
        .collect();
    assert_eq!(
        tails,
        vec![(Some("base.Make()"), true), (Some("this.Make()"), false)],
        "a base-qualified chain head marks its tail, so the resolver starts the \
             method-return hop at the bases; a this-qualified one keeps hopping through the \
             enclosing type"
    );
    for r in e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Validate"))
    {
        assert_eq!(
            r.receiver_call_owner.as_deref(),
            Some("Use"),
            "both heads type as the enclosing type -- the marker is what tells them apart"
        );
        assert_eq!(r.receiver_call_member.as_deref(), Some("Make"));
    }
}

#[test]
fn stage4_second_hop_of_a_chain_emits_no_ref() {
    let e = extract_src(
        r#"
namespace App.ChainOfChains;

public class Host
{
  private Widget a;

  public void Run()
  {
    var y = a.B().C().D();
  }
}
"#,
    );
    // `.D`'s OWN qualifier is `a.B().C()`, itself a chain (its `function`
    // is a member_access_expression whose own `expression` is the
    // invocation `a.B()`, not a plain name) -- `member_qualifier_info`
    // has no arm for an `invocation_expression` qualifier, so `.D` never
    // reaches the chain-tail rule at all and earns no ref, precise or
    // otherwise.
    let d = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("D"));
    assert!(
        d.is_none(),
        "a chain-of-chains qualifier earns no ref for its own tail"
    );

    // The NESTED `.C` window, walked independently, is an ordinary
    // chain tail in its own right and still earns its own ref.
    let c = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("C"))
        .expect("the nested a.B().C() window still earns its own ref");
    assert_eq!(c.receiver_call_owner.as_deref(), Some("Widget"));
    assert_eq!(c.receiver_call_member.as_deref(), Some("B"));
}

#[test]
fn stage4_first_single_parameter_lambda_on_a_collection_receiver_gets_the_element_type() {
    let e = extract_src(
        r#"
namespace App.LambdaElement;

public class Host
{
  private Widget[] arr;
  private List<Widget> list;

  public void Run()
  {
    arr.Where(x => x.Go());
    list.Where((y) => y.Go());
  }
}
"#,
    );
    let go_types: Vec<_> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Go"))
        .map(|r| r.receiver_type.as_deref())
        .collect();
    assert_eq!(
        go_types,
        vec![Some("Widget"), Some("Widget")],
        "an array's implicit lambda parameter (`x`) and a single-argument generic's \
             parenthesized untyped parameter (`(y)`) both earn the element type"
    );
}

#[test]
fn stage4_lambda_on_a_two_argument_generic_receiver_gets_no_fact() {
    let e = extract_src(
        r#"
namespace App.LambdaNoFact;

public class Host
{
  private Dictionary<string, Widget> dict;

  public void Run()
  {
    dict.Where(kv => kv.Deconstruct());
  }
}
"#,
    );
    let call = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Deconstruct"))
        .expect("kv.Deconstruct() still earns an ordinary ref");
    assert_eq!(
        call.receiver_type, None,
        "a two-argument generic receiver (Dictionary<K,V>) never types the lambda parameter"
    );
}
