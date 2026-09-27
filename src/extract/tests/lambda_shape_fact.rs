use super::*;

// (valueOnly, implicit) at each argument position of the `Take` call on
// `line`, `None` where no positional lambda literal sits.
fn take_shape(e: &Extraction, line: usize) -> Option<Vec<Option<(bool, bool)>>> {
    e.refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Take") && r.line == line)
        .unwrap_or_else(|| panic!("no Take ref on line {line}"))
        .lambda_arg_shape
        .as_ref()
        .map(|shapes| {
            shapes
                .iter()
                .map(|s| s.map(|s| (s.value_only, s.implicitly_typed)))
                .collect()
        })
}

fn host(body: &str) -> Extraction {
    extract_src(&format!(
        "namespace App.Groups;\n\npublic class Host\n{{\n  public void Run(Sink s, int y)\n  {{\n{body}\n  }}\n}}\n"
    ))
}

#[test]
fn a_body_that_can_only_produce_a_value_is_value_only() {
    let e = host(
        r#"    s.Take(_ => true);
    s.Take(m => m switch { A a => true, _ => false });
    s.Take(x => x.P);
    s.Take(x => { return 1; });
    s.Take(async x => true);
    s.Take(x => (x.M()));
    s.Take(x => !x.Ok);"#,
    );
    for line in 7..=13 {
        assert_eq!(
            take_shape(&e, line),
            Some(vec![Some((true, true))]),
            "line {line}"
        );
    }
}

#[test]
fn a_body_c_sharp_can_use_as_a_statement_is_not_value_only() {
    let e = host(
        r#"    s.Take(x => x.M());
    s.Take(x => y = x);
    s.Take(x => y += x);
    s.Take(x => new T());
    s.Take(x => throw new E());
    s.Take(x => { M(); });
    s.Take(x => { Use(() => { return 1; }); });
    s.Take(x => y++);
    s.Take(async x => await x.Run());
    s.Take(x => x?.M());
    s.Take(x => checked(x + 1));"#,
    );
    for line in 7..=17 {
        assert_eq!(
            take_shape(&e, line),
            Some(vec![Some((false, true))]),
            "line {line}"
        );
    }
}

#[test]
fn a_return_inside_a_nested_local_function_belongs_to_that_function() {
    let e = host(r#"    s.Take(x => { int F() { return 1; } F(); });"#);
    assert_eq!(take_shape(&e, 7), Some(vec![Some((false, true))]));
}

#[test]
fn written_parameter_types_make_the_lambda_explicitly_typed() {
    let e = host(
        r#"    s.Take((int x) => true);
    s.Take(x => true);
    s.Take((a, b) => true);
    s.Take((int a, int b) => true);
    s.Take(() => true);"#,
    );
    assert_eq!(take_shape(&e, 7), Some(vec![Some((true, false))]));
    assert_eq!(take_shape(&e, 8), Some(vec![Some((true, true))]));
    assert_eq!(take_shape(&e, 9), Some(vec![Some((true, true))]));
    assert_eq!(take_shape(&e, 10), Some(vec![Some((true, false))]));
    assert_eq!(
        take_shape(&e, 11),
        Some(vec![Some((true, true))]),
        "no parameter carries a written type"
    );
}

#[test]
fn named_arguments_method_groups_and_anonymous_methods_record_nothing() {
    let e = host(
        r#"    object One(int a) => a;
    s.Take(filter: x => true);
    s.Take(One);
    s.Take(delegate (int a) { return true; });
    s.Take(7, x => true);
    s.Take(One, x => x.M(), filter: x => true);"#,
    );
    assert_eq!(take_shape(&e, 8), None);
    assert_eq!(
        take_shape(&e, 9),
        None,
        "a local function passed as a method group has an arity fact but no body fact"
    );
    assert_eq!(take_shape(&e, 10), None);
    assert_eq!(take_shape(&e, 11), Some(vec![None, Some((true, true))]));
    assert_eq!(
        take_shape(&e, 12),
        Some(vec![None, Some((false, true)), None])
    );
}

#[test]
fn the_arity_fact_is_unchanged_beside_the_shape() {
    let e = host(r#"    s.Take(7, (a, b) => true);"#);
    let r = e
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Take"))
        .expect("Take ref");
    assert_eq!(r.lambda_arg_arity, Some(vec![None, Some(2)]));
}

#[test]
fn a_chain_tail_and_a_conditional_access_call_carry_their_own_shape() {
    let e = host(
        r#"    s.Make().Take(x => true);
    s?.Take(x => x.M());"#,
    );
    assert_eq!(take_shape(&e, 7), Some(vec![Some((true, true))]));
    assert_eq!(take_shape(&e, 8), Some(vec![Some((false, true))]));
}

#[test]
fn extract_dump_carries_the_shape() {
    let e = host(r#"    s.Take(7, x => true);"#);
    let json = crate::extract::extraction_to_json(&e);
    assert!(
        json.contains("\"lambdaArgShape\": [\n")
            && json.contains("\"valueOnly\": true")
            && json.contains("\"implicit\": true"),
        "{json}"
    );
}
