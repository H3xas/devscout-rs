use super::*;

// A qualified name written with whitespace, a line break or a comment
// between its segments must read exactly as its clean spelling does: every
// dotted string the extractor records is rebuilt from the name's own tokens,
// so no consumer downstream -- the last-dot split, a def-path suffix test, a
// scope lookup, a def id -- ever sees the trivia.

/// The same dotted name written four ways.
const WRITINGS: [&str; 4] = [
    "Widgets . Widget",
    "Widgets./*c*/Widget",
    "Widgets. // c\n        Widget",
    "Widgets.\n        Widget",
];

fn declaration(written: &str) -> String {
    format!(
        "namespace App;\n\npublic class Host\n{{\n    private {written} _field;\n\n    \
         public void Run({written} parameter)\n    {{\n        {written} local = null;\n        \
         _field.Go();\n        parameter.Go();\n        local.Go();\n    }}\n}}\n"
    )
}

fn member_ref<'a>(e: &'a Extraction, name: &str) -> &'a RefRecord {
    e.refs
        .iter()
        .find(|r| r.kind == "uses-member" && r.name == name)
        .unwrap_or_else(|| panic!("no uses-member ref for {name}"))
}

fn field_fact<'a>(def: &'a DefRecord, field: &str) -> Option<&'a Fact> {
    def.field_types
        .iter()
        .find(|(name, _)| name == field)
        .map(|(_, fact)| fact)
}

fn qualified_type_refs(e: &Extraction) -> Vec<(String, Option<String>)> {
    e.refs
        .iter()
        .filter(|r| r.kind == "uses-type" && r.name == "Widget")
        .map(|r| (r.name.clone(), r.qualified.clone()))
        .collect()
}

#[test]
fn qualified_path_tokens_ac_1_a_receivers_written_type_is_rebuilt_from_its_tokens() {
    for written in WRITINGS {
        let e = extract_src(&declaration(written));
        for receiver in ["_field", "parameter", "local"] {
            let r = member_ref(&e, receiver);
            assert_eq!(
                r.receiver_type_qualified.as_deref(),
                Some("Widgets.Widget"),
                "{written:?} on {receiver}"
            );
            assert_eq!(r.receiver_type.as_deref(), Some("Widget"), "{written:?}");
        }
    }
}

#[test]
fn qualified_path_tokens_ac_1_a_trivia_written_field_records_its_qualified_type_in_the_dump() {
    let e = extract_src(&declaration("Widgets . Widget"));
    let host = find_def(&e, "App.Host").expect("host def");
    assert_eq!(
        field_fact(host, "_field").and_then(|f| f.qualified_type.as_deref()),
        Some("Widgets.Widget")
    );

    let e = extract_src(
        "namespace App;\n\npublic class Host\n{\n    public Widgets . Widget Prop { get; }\n}\n",
    );
    let dump: serde_json::Value =
        serde_json::from_str(&extraction_to_json(&e)).expect("dump is JSON");
    assert_eq!(
        dump["defs"][0]["propertyTypes"]["Prop"]["qualifiedType"],
        "Widgets.Widget"
    );
}

#[test]
fn qualified_path_tokens_ac_1_nullable_array_and_alias_wrappers_read_as_their_clean_spellings() {
    let e = extract_src(
        "namespace App;\n\npublic class Host\n{\n    private Widgets . Widget? _nullable;\n    \
         private Widgets . Widget[] _array;\n    private global :: App.Widgets.Widget _aliased;\n}\n",
    );
    let host = find_def(&e, "App.Host").expect("host def");
    let written = |field: &str| field_fact(host, field).and_then(|f| f.qualified_type.clone());
    assert_eq!(written("_nullable").as_deref(), Some("Widgets.Widget"));
    assert_eq!(written("_array").as_deref(), Some("Widgets.Widget"));
    assert_eq!(
        written("_aliased").as_deref(),
        Some("global::App.Widgets.Widget")
    );
}

#[test]
fn qualified_path_tokens_ac_2_a_type_reference_reads_the_same_dotted_name() {
    for written in WRITINGS {
        let e = extract_src(&declaration(written));
        assert_eq!(
            qualified_type_refs(&e)
                .into_iter()
                .filter(|(_, qualified)| qualified.as_deref() == Some("Widgets.Widget"))
                .count(),
            2,
            "{written:?} records the field and the parameter; a local is a receiver fact only"
        );
    }
}

#[test]
fn qualified_path_tokens_ac_2_a_qualified_type_argument_reads_the_same_dotted_name() {
    for written in WRITINGS {
        let e = extract_src(&format!(
            "namespace App;\n\npublic class Host\n{{\n    private List<{written}> _items;\n}}\n"
        ));
        assert!(
            qualified_type_refs(&e)
                .iter()
                .any(|(_, qualified)| qualified.as_deref() == Some("Widgets.Widget")),
            "{written:?} records the type argument through its tokens"
        );
    }
}

#[test]
fn qualified_path_tokens_ac_2_a_base_list_entry_reads_the_same_dotted_name() {
    let e = extract_src("namespace App;\n\npublic class Derived : Widgets . /*c*/ Base { }\n");
    let base = e
        .refs
        .iter()
        .find(|r| r.kind == "inherits")
        .expect("base ref present");
    assert_eq!(base.name, "Base");
    assert_eq!(base.qualified.as_deref(), Some("Widgets.Base"));
}

#[test]
fn qualified_path_tokens_ac_2_a_registration_reads_the_same_dotted_names() {
    let e = extract_src(
        "namespace App;\n\npublic class Startup\n{\n    public void Configure(IServiceCollection \
         services)\n    {\n        services.AddScoped<Widgets . IWidget, \
         Widgets./*c*/Widget>();\n    }\n}\n",
    );
    let registration = e
        .registrations
        .first()
        .unwrap_or_else(|| panic!("no registration recorded"));
    assert_eq!(registration.service, "Widgets.IWidget");
    assert_eq!(registration.implementation, "Widgets.Widget");
}

#[test]
fn qualified_path_tokens_ac_3_a_namespace_name_reads_as_its_clean_spelling() {
    let block = extract_src("namespace App . Widgets\n{\n    public class Widget { }\n}\n");
    assert!(find_def(&block, "App.Widgets.Widget").is_some());

    let file_scoped = extract_src("namespace App./*c*/Widgets;\n\npublic class Widget { }\n");
    assert!(find_def(&file_scoped, "App.Widgets.Widget").is_some());
}

#[test]
fn qualified_path_tokens_ac_3_a_using_target_reads_as_its_clean_spelling() {
    let plain = extract_src("using App . Widgets;\n\nnamespace App;\n\npublic class Host { }\n");
    assert_eq!(using_texts(&plain), vec!["App.Widgets"]);

    let statics = extract_src(
        "using static App . /*c*/ Widgets.Tools;\n\nnamespace App;\n\npublic class Host { }\n",
    );
    assert_eq!(using_texts(&statics), vec!["App.Widgets.Tools"]);

    let global =
        extract_src("global using App.\n    Widgets;\n\nnamespace App;\n\npublic class Host { }\n");
    let UsingRecord::Plain { text, global, .. } = &global.usings[0] else {
        panic!("a global using is a plain using");
    };
    assert_eq!(text, "App.Widgets");
    assert!(*global);
}

#[test]
fn qualified_path_tokens_ac_3_a_using_alias_reads_as_its_clean_spelling() {
    let e =
        extract_src("using W = App . Widgets.Widget;\n\nnamespace App;\n\npublic class Host { }\n");
    let UsingRecord::Alias { alias, target, .. } = &e.usings[0] else {
        panic!("an alias directive records an alias");
    };
    assert_eq!(alias, "W");
    assert_eq!(target, "App.Widgets.Widget");
}

fn using_texts(e: &Extraction) -> Vec<&str> {
    e.usings
        .iter()
        .filter_map(|u| match u {
            UsingRecord::Plain { text, .. } => Some(text.as_str()),
            UsingRecord::Alias { .. } => None,
        })
        .collect()
}

#[test]
fn qualified_path_tokens_ac_5_a_malformed_name_keeps_its_source_text() {
    // Each source writes a name this grammar cannot parse as a dotted path.
    // The recorded value is the raw text, so the name earns no path the
    // source never spelled and binds nothing new.

    let using_ = extract_src("using App..Widgets;\n\nnamespace App;\n\npublic class Host { }\n");
    assert_eq!(using_texts(&using_), vec!["App..Widgets"]);

    let field =
        extract_src("namespace App;\n\npublic class Host\n{\n    private Widgets..Widget _w;\n}\n");
    let qualified: Vec<Option<&str>> = field
        .refs
        .iter()
        .filter(|r| r.kind == "uses-type")
        .map(|r| r.qualified.as_deref())
        .collect();
    assert_eq!(qualified, vec![Some("Widgets..Widget")]);

    let namespace = extract_src("namespace App.*Widgets;\n\npublic class Host { }\n");
    assert!(
        find_def(&namespace, "App.*Widgets.Host").is_some(),
        "a namespace name that did not parse keeps its source text"
    );
}

#[test]
fn qualified_path_tokens_ac_2_a_generic_tail_written_with_argument_spacing_keeps_its_prefix() {
    // A qualified name's tail may itself carry a type-argument list, and the
    // tail's raw text is not what the name's own token path ends with once
    // that list is written with a space after its comma. Splitting with the
    // tail's own reader keeps the prefix, so the reference is recorded at
    // all rather than dropped.
    let e = extract_src(
        "namespace App;\n\npublic class Host\n{\n    private Outer<T>.Proxy<TData, TException> _p;\n}\n",
    );
    let qualified: Vec<&str> = e
        .refs
        .iter()
        .filter(|r| r.kind == "uses-type")
        .filter_map(|r| r.qualified.as_deref())
        .collect();
    assert_eq!(qualified, vec!["Outer<T>.Proxy"]);
}
