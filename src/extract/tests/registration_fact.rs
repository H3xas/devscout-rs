use super::*;

// --- the registration shape rule -------------------------------------------

#[test]
fn a_two_type_argument_add_scoped_call_records_a_registration_fact() {
    let e = extract_src(
        "namespace App.Wiring;\n\nclass Startup\n{\n  void Configure()\n  {\n    services.AddScoped<IContract, Widget>();\n  }\n}\n",
    );
    assert_eq!(e.registrations.len(), 1);
    let reg = &e.registrations[0];
    assert_eq!(reg.service, "IContract");
    assert_eq!(reg.implementation, "Widget");
    assert_eq!(reg.namespace, "App.Wiring");
    assert_eq!(reg.line, 7);
}

#[test]
fn the_keyed_and_named_spellings_match_by_construction_with_no_dedicated_rule() {
    let e = extract_src(
        "namespace App.Wiring;\n\nclass Startup\n{\n  void Configure()\n  {\n    services.AddKeyedSingleton<IContract, Widget>(\"primary\");\n    services.TryAddScoped<IOther, Other>();\n    services.TryAddTransient<IThird, Third>();\n  }\n}\n",
    );
    let names: Vec<(&str, &str)> = e
        .registrations
        .iter()
        .map(|r| (r.service.as_str(), r.implementation.as_str()))
        .collect();
    assert_eq!(
        names,
        vec![
            ("IContract", "Widget"),
            ("IOther", "Other"),
            ("IThird", "Third"),
        ],
        "the prefix/suffix shape rule covers Add/TryAdd and the Keyed spelling with no name list of its own"
    );
}

#[test]
fn a_one_type_argument_registration_call_records_nothing() {
    let e = extract_src(
        "namespace App.Wiring;\n\nclass Startup\n{\n  void Configure()\n  {\n    services.AddSingleton<Widget>();\n  }\n}\n",
    );
    assert!(
        e.registrations.is_empty(),
        "one type argument is not the two-type-argument registration shape"
    );
}

#[test]
fn a_call_whose_name_does_not_match_the_add_try_add_prefix_or_the_lifetime_suffix_records_nothing()
{
    let e = extract_src(
        "namespace App.Wiring;\n\nclass Startup\n{\n  void Configure()\n  {\n    services.Configure<IContract, Widget>();\n  }\n}\n",
    );
    assert!(e.registrations.is_empty());
}

#[test]
fn a_generic_method_reference_that_is_never_invoked_records_nothing() {
    let e = extract_src(
        "namespace App.Wiring;\n\nclass Startup\n{\n  void Configure()\n  {\n    var m = services.AddScoped<IContract, Widget>;\n  }\n}\n",
    );
    assert!(
        e.registrations.is_empty(),
        "the shape rule requires an actual invocation, not a bare method-group reference"
    );
}
