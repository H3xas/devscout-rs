use super::*;

fn arg_types(source: &str, member: &str) -> Vec<Option<ArgTypeRecord>> {
    extract_src(source)
        .refs
        .into_iter()
        .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some(member))
        .and_then(|r| r.arg_types)
        .expect("call fact")
}

#[test]
fn syntax_arguments_keep_their_written_positions() {
    let source = r#"
class C {
  void Run() => this.Use("text", $"{1}", 'c', true, null, 1, 1u, 1L, 1.0f, 1.0m,
                         typeof(C), new C(), (C)this, this as C, unknown);
  void Use(params object[] values) { }
}
"#;
    let facts = arg_types(source, "Use");
    let names: Vec<Option<String>> = facts
        .into_iter()
        .map(|f| match f {
            Some(ArgTypeRecord::Known(name)) => Some(name),
            _ => None,
        })
        .collect();
    assert_eq!(
        names,
        vec![
            Some("string".into()),
            Some(INTERPOLATED_ARG_DESCRIPTOR.into()),
            Some("char".into()),
            Some("bool".into()),
            Some("null".into()),
            Some("int".into()),
            Some("uint".into()),
            Some("long".into()),
            Some("float".into()),
            Some("decimal".into()),
            Some("Type".into()),
            Some("C".into()),
            Some("C".into()),
            Some("C".into()),
            None,
        ]
    );
}

#[test]
fn declarations_and_members_supply_types_without_turning_unknowns_into_guesses() {
    let source = r#"
class Box<T> {
  string field;
  public string Property { get; }
  public string Get() => "ok";
  void Run(string parameter, Box<string> box) {
    string local = "a";
    var inferred = "b";
    var created = new Box<string>();
    this.Use(parameter, field, local, inferred, created, box.Property, box.Get(),
             Get(), TimeSpan.FromSeconds(1), CancellationToken.None);
  }
  void Use(params object[] values) { }
}
"#;
    let facts = arg_types(source, "Use");
    assert!(facts.iter().take(5).all(|f| f.is_some()), "{facts:?}");
    assert!(
        matches!(&facts[5], Some(ArgTypeRecord::Member { member, call: false, .. }) if member == "Property")
    );
    assert!(
        matches!(&facts[6], Some(ArgTypeRecord::Member { member, call: true, .. }) if member == "Get")
    );
    assert!(
        matches!(&facts[8], Some(ArgTypeRecord::Member { owner, member, call: true, .. }) if owner == "TimeSpan" && member == "FromSeconds")
    );
    assert!(
        matches!(&facts[9], Some(ArgTypeRecord::Member { owner, member, call: false, .. }) if owner == "CancellationToken" && member == "None")
    );
}

#[test]
fn named_and_by_reference_arguments_do_not_claim_positional_types() {
    let named = extract_src("class C { void Run() => this.Use(second: 1, first: \"s\"); void Use(int first, string second) {} }");
    assert!(named
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Use"))
        .unwrap()
        .arg_types
        .is_none());
    let by_ref = arg_types("class C { void Run(ref int x) => this.Use(ref x, out x, in x, x => x); void Use(params object[] args) {} }", "Use");
    assert!(by_ref.iter().all(Option::is_none));
}

#[test]
fn direct_conditional_and_chain_calls_keep_their_own_argument_lists() {
    let source = r#"
class C {
  C Get() => this;
  void Use(object value) { }
  void Run(C other) {
    other.Use(1);
    other?.Use("conditional");
    other.Get().Use(true);
  }
}
"#;
    let facts: Vec<_> = extract_src(source)
        .refs
        .into_iter()
        .filter(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Use"))
        .map(|r| r.arg_types.expect("the callee's arguments"))
        .collect();
    assert_eq!(
        facts,
        vec![
            vec![Some(ArgTypeRecord::Known("int".into()))],
            vec![Some(ArgTypeRecord::Known("string".into()))],
            vec![Some(ArgTypeRecord::Known("bool".into()))],
        ]
    );
}

#[test]
fn a_method_group_is_unknown_and_a_local_shadows_a_field() {
    let source = r#"
class C {
  string value;
  void Work() { }
  void Use(object first, object second) { }
  void Run() {
    int value = 1;
    this.Use(Work, value);
  }
}
"#;
    assert_eq!(
        arg_types(source, "Use"),
        vec![None, Some(ArgTypeRecord::Known("int".into()))]
    );
}

#[test]
fn argument_typing_does_not_add_predefined_constructor_di_facts() {
    let source =
        "class C { public C(int count, string name, IService service) {} } interface IService {}";
    let ctor_types: Vec<_> = extract_src(source)
        .refs
        .into_iter()
        .filter(|r| r.kind == "ctor-param")
        .map(|r| r.name)
        .collect();
    assert_eq!(ctor_types, vec!["IService"]);
}

#[test]
fn typeof_inferred_local_is_argument_only() {
    let source = "class C { void Use(object value) {} void Run() { var type = typeof(C); this.Use(type); type.IsNullable(1); } }";
    let extracted = extract_src(source);
    let use_ref = extracted
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("Use"))
        .unwrap();
    assert_eq!(
        use_ref.arg_types,
        Some(vec![Some(ArgTypeRecord::Known("Type".into()))])
    );
    let receiver_ref = extracted
        .refs
        .iter()
        .find(|r| r.member.as_deref() == Some("IsNullable"))
        .unwrap();
    assert_eq!(receiver_ref.receiver_type, None);
}

#[test]
fn interpolated_handler_ac_2_interpolated_arguments_are_told_apart_from_strings() {
    let source = r#"
class Box { public string Value { get; } public string Get() => "g"; }
class C {
  string field;
  void Run(string parameter, Box box) {
    string local = "l";
    this.Use($"a", $"b{1}", $@"v", @$"w", $"""raw""", ($"paren"),
              "plain", @"verbatim", """raw""",
              local, parameter, field, box.Value, box.Get(), "x" + $"y");
  }
  void Use(params object[] values) { }
}
"#;
    let facts = arg_types(source, "Use");
    let names: Vec<Option<String>> = facts
        .iter()
        .take(12)
        .map(|f| match f {
            Some(ArgTypeRecord::Known(name)) => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        names,
        vec![
            Some(INTERPOLATED_ARG_DESCRIPTOR.into()),
            Some(INTERPOLATED_ARG_DESCRIPTOR.into()),
            Some(INTERPOLATED_ARG_DESCRIPTOR.into()),
            Some(INTERPOLATED_ARG_DESCRIPTOR.into()),
            Some(INTERPOLATED_ARG_DESCRIPTOR.into()),
            Some(INTERPOLATED_ARG_DESCRIPTOR.into()),
            Some("string".into()),
            Some("string".into()),
            Some("string".into()),
            Some("string".into()),
            Some("string".into()),
            Some("string".into()),
        ]
    );
    assert!(
        matches!(&facts[12], Some(ArgTypeRecord::Member { member, .. }) if member == "Value"),
        "{:?}",
        facts[12]
    );
    assert!(
        matches!(&facts[13], Some(ArgTypeRecord::Member { member, .. }) if member == "Get"),
        "{:?}",
        facts[13]
    );
    assert!(
        facts[14].is_none(),
        "a concatenation is not one interpolated argument"
    );
}
