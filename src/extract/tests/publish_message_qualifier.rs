use super::*;

// A publish message the call itself writes keeps the qualifier it was
// written with, so the resolver reads `Subjects.StartCommand` through the
// qualified steps instead of looking the bare `StartCommand` up from the
// call site's namespace.

fn messages(src: &str) -> Vec<String> {
    let body = format!("namespace App.Bus;\n\nclass Clerk\n{{\n{src}\n}}\n");
    extract_src(&body)
        .publishes
        .into_iter()
        .map(|p| p.message)
        .collect()
}

fn message_of(verb: &str, src: &str) -> String {
    let body = format!("namespace App.Bus;\n\nclass Clerk\n{{\n{src}\n}}\n");
    let e = extract_src(&body);
    let found: Vec<&PublishRecord> = e.publishes.iter().filter(|p| p.verb == verb).collect();
    assert_eq!(found.len(), 1, "{:?}", e.publishes);
    found[0].message.clone()
}

#[test]
fn a_qualified_type_argument_keeps_its_qualifier() {
    assert_eq!(
        messages("  void A(IBus bus) { bus.Publish<Ns.M>(default); }"),
        vec!["Ns.M"]
    );
}

#[test]
fn a_qualified_constructed_message_keeps_its_qualifier() {
    assert_eq!(
        messages("  void A(IBus bus) { bus.Publish(new Ns.M(), default); }"),
        vec!["Ns.M"]
    );
}

#[test]
fn a_qualified_message_constructed_inside_a_lambda_argument_keeps_its_qualifier() {
    assert_eq!(
        message_of(
            "PublishAsync",
            "  void A(IBus bus) { bus.PublishAsync(context => context.Init(new Ns.M())); }"
        ),
        "Ns.M"
    );
}

#[test]
fn a_qualified_explicit_array_creation_keeps_its_qualifier() {
    assert_eq!(
        messages("  void A(IBus bus) { bus.Publish(new Ns.M[2], default); }"),
        vec!["Ns.M[]"]
    );
    assert_eq!(
        messages("  void A(IBus bus) { bus.Publish(new Ns.M[] { null }, default); }"),
        vec!["Ns.M[]"]
    );
}

#[test]
fn a_qualified_implicit_array_element_keeps_its_qualifier() {
    assert_eq!(
        message_of(
            "Publish",
            "  void A(IBus bus) { bus.Publish(new[] { new Ns.M(), new Ns.M() }, default); }"
        ),
        "Ns.M[]"
    );
}

#[test]
fn implicit_array_elements_spelled_differently_keep_the_bare_rendering() {
    assert_eq!(
        message_of(
            "Publish",
            "  void A(IBus bus) { bus.Publish(new[] { new Ns.M(), new M() }, default); }"
        ),
        "M[]"
    );
}

#[test]
fn a_nested_type_named_through_its_outer_type_keeps_the_outer_name() {
    assert_eq!(
        messages("  void A(IBus bus) { bus.Publish(new Outer.Inner(), default); }"),
        vec!["Outer.Inner"]
    );
}

#[test]
fn a_qualified_generic_message_keeps_its_qualifier_and_bare_type_arguments() {
    assert_eq!(
        messages("  void A(IBus bus) { bus.Publish<Ns.Batch<Other.M>>(default); }"),
        vec!["Ns.Batch<M>"]
    );
}

#[test]
fn a_nullable_qualified_message_keeps_its_qualifier() {
    assert_eq!(
        messages("  void A(IBus bus) { bus.Publish<Ns.M?>(default); }"),
        vec!["Ns.M"]
    );
}

#[test]
fn the_qualifier_matches_the_one_the_type_reference_records() {
    let e = extract_src(
        "namespace App.Bus;\n\nclass Clerk\n{\n  void A(IBus bus) { bus.Publish(new A.B.M(), default); }\n}\n",
    );
    assert_eq!(e.publishes[0].message, "A.B.M");
    let written: Vec<Option<&str>> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-type" && r.name == "M")
        .map(|r| r.qualified.as_deref())
        .collect();
    assert_eq!(written, vec![Some("A.B.M")]);
}

#[test]
fn an_alias_qualified_message_keeps_the_text_the_type_reference_records() {
    assert_eq!(
        messages("  void A(IBus bus) { bus.Publish(new global::Ns.M(), default); }"),
        vec!["global::Ns.M"]
    );
    assert_eq!(
        messages("  void A(IBus bus) { bus.Publish(new global::M(), default); }"),
        vec!["M"]
    );
}

#[test]
fn whitespace_around_the_qualifier_dots_is_not_part_of_the_message() {
    assert_eq!(
        messages("  void A(IBus bus) { bus.Publish(new Ns .\n    Sub . M(), default); }"),
        vec!["Ns.Sub.M"]
    );
}

#[test]
fn a_bare_message_is_unchanged() {
    assert_eq!(
        messages("  void A(IBus bus) { bus.Publish(new M(), default); }"),
        vec!["M"]
    );
}

#[test]
fn a_message_read_off_an_identifier_argument_is_unchanged() {
    assert_eq!(
        message_of(
            "Publish",
            "  void A(IBus bus) { var msg = new Ns.M(); bus.Publish(msg, default); }"
        ),
        "M"
    );
}

#[test]
fn a_pass_through_message_is_unchanged() {
    assert_eq!(
        message_of(
            "Publish",
            "  void Forward<T>(IBus bus, T m) { bus.Publish<T>(m); }"
        ),
        "*"
    );
}

#[test]
fn consumer_side_and_parameter_texts_are_unchanged() {
    let e = extract_src(
        "namespace App.Bus;\n\nclass Handler : IConsumer<Ns.M>\n{\n  public void Consume(Ns.M m) { }\n}\n",
    );
    let def = find_def(&e, "App.Bus.Handler").expect("handler def");
    assert_eq!(
        def.method_params,
        vec![("Consume".to_string(), vec![vec!["M".to_string()]])]
    );
    assert_eq!(
        def.base_generic_args,
        vec![("IConsumer".to_string(), vec!["M".to_string()])]
    );
}

#[test]
fn extract_dump_prints_the_qualified_message() {
    let json = extraction_to_json(&extract_src(
        "namespace App.Bus;\n\nclass Clerk\n{\n  void A(IBus bus) { bus.Publish(new Ns.M(), default); }\n}\n",
    ));
    let dump: serde_json::Value = serde_json::from_str(&json).expect("dump is JSON");
    assert_eq!(dump["publishes"][0]["message"], "Ns.M");
}
