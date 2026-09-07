use super::*;

// --- LambdaSlot: the site-side untyped-lambda callee slot ---------------

#[test]
fn stage4_untyped_lambda_argument_carries_its_callee_slot() {
    let e = extract_src(
        r#"
namespace App.LambdaSlots;

public class Host
{
    private Registrar reg;

    public void RunOne()
    {
        reg.Register(x => x.Configure());
    }

    public void RunTwo()
    {
        Register2(x => { x.Configure(); });
    }

    public void RunThree()
    {
        this.Register3(x => x.Configure());
    }
}
"#,
    );
    let slots: Vec<_> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Configure"))
        .collect();
    assert_eq!(slots.len(), 3, "one Configure ref per call site");
    let expected = [
        LambdaSlot {
            owner: "Registrar".to_string(),
            member: "Register".to_string(),
            arg_count: 1,
            arg_index: 0,
            arity: 1,
            index: 0,
        },
        LambdaSlot {
            owner: "Host".to_string(),
            member: "Register2".to_string(),
            arg_count: 1,
            arg_index: 0,
            arity: 1,
            index: 0,
        },
        LambdaSlot {
            owner: "Host".to_string(),
            member: "Register3".to_string(),
            arg_count: 1,
            arg_index: 0,
            arity: 1,
            index: 0,
        },
    ];
    for (r, want) in slots.iter().zip(expected.iter()) {
        assert_eq!(r.receiver_lambda.as_ref(), Some(want));
        assert_eq!(r.receiver_type, None);
        assert!(r.receiver_local, "the parameter is a member-scoped name");
    }
}

#[test]
fn stage4_lambda_slot_records_the_position_of_a_two_parameter_lambda() {
    let e = extract_src(
        r#"
namespace App.LambdaSlots2;

public class Host
{
    public void Run()
    {
        Bus.Configure("q", (ctx, cfg) => cfg.Bind());
    }
}
"#,
    );
    let bind = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Bind"))
        .expect("cfg.Bind() still earns an ordinary ref");
    assert_eq!(
        bind.receiver_lambda,
        Some(LambdaSlot {
            owner: "Bus".to_string(),
            member: "Configure".to_string(),
            arg_count: 2,
            arg_index: 1,
            arity: 2,
            index: 1,
        })
    );
}

#[test]
fn stage4_lambda_slot_yields_to_the_collection_element_rule() {
    let e = extract_src(
        r#"
namespace App.LambdaSlots3;

public class Host
{
    private List<Order> orders;

    public void Run()
    {
        orders.Where(o => o.Validate());
    }
}
"#,
    );
    let validate = e
        .refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Validate"))
        .expect("o.Validate() still earns an ordinary ref");
    assert_eq!(validate.receiver_type.as_deref(), Some("Order"));
    assert_eq!(validate.receiver_lambda, None);
}

#[test]
fn stage4_lambda_slot_is_refused_for_named_chained_and_generic_callees() {
    let e = extract_src(
        r#"
namespace App.LambdaSlots4;

public class Host
{
    private Registrar reg;

    public void Run()
    {
        reg.Register(configure: x => x.A());
        reg.Build().Register(x => x.B());
        reg.Register<Options>(x => x.C());
    }
}
"#,
    );
    for member in ["A", "B", "C"] {
        let r = e
            .refs
            .iter()
            .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some(member))
            .unwrap_or_else(|| panic!("{member} ref present"));
        assert_eq!(r.receiver_lambda, None, "{member} carries no slot");
        assert_eq!(
            r.receiver_type, None,
            "{member} carries no receiver type either"
        );
    }
}

#[test]
fn stage4_two_lambdas_with_different_callees_and_one_name_conflict_to_no_slot() {
    let e = extract_src(
        r#"
namespace App.LambdaSlots5;

public class Host
{
    private Registrar reg;
    private Widget other;

    public void Run()
    {
        reg.Register(x => x.A());
        other.Attach(x => x.B());
    }
}
"#,
    );
    for member in ["A", "B"] {
        let r = e
            .refs
            .iter()
            .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some(member))
            .unwrap_or_else(|| panic!("{member} ref present"));
        assert_eq!(
            r.receiver_lambda, None,
            "{member}'s parameter name conflicts across two different callees"
        );
    }
}
