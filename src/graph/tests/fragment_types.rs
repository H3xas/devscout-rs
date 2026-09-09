use super::*;
use crate::extract;

// --- FragDef's new keys, appended last, omitted when empty ----------------------

fn frag_def(
    methods: &[&str],
    properties: &[&str],
    fields: &[&str],
    method_returns: &[(&str, &str)],
    extension_methods: &[(&str, &str, usize, i64)],
) -> FragDef {
    frag_def_with_bases(
        methods,
        properties,
        fields,
        method_returns,
        extension_methods,
        &[],
    )
}

fn frag_def_with_bases(
    methods: &[&str],
    properties: &[&str],
    fields: &[&str],
    method_returns: &[(&str, &str)],
    extension_methods: &[(&str, &str, usize, i64)],
    bases: &[&str],
) -> FragDef {
    let mut mr = OrderedMap::new();
    for (k, v) in method_returns {
        mr.insert((*k).to_string(), (*v).to_string());
    }
    FragDef {
        id: "App.Facts.Widget".into(),
        name: "Widget".into(),
        namespace: "App.Facts".into(),
        kind: "class".into(),
        line: 3,
        methods: methods.iter().map(|s| s.to_string()).collect(),
        properties: properties.iter().map(|s| s.to_string()).collect(),
        fields: fields.iter().map(|s| s.to_string()).collect(),
        method_returns: mr,
        extension_methods: extension_methods
            .iter()
            .map(|(n, t, lo, hi)| FragExtensionMethod {
                name: (*n).to_string(),
                this_type: (*t).to_string(),
                arity_min: *lo,
                arity_max: *hi,
                this_args: None,
            })
            .collect(),
        bases: bases.iter().map(|s| s.to_string()).collect(),
        type_params: Vec::new(),
        base_generic_args: OrderedMap::new(),
        property_types: OrderedMap::new(),
        field_types: OrderedMap::new(),
        method_return_args: OrderedMap::new(),
        non_public_methods: Vec::new(),
        method_arities: OrderedMap::new(),
        method_params: OrderedMap::new(),
        override_methods: Vec::new(),
        test_methods: Vec::new(),
        end_line: 0,
    }
}

#[test]
fn frag_def_appends_properties_fields_and_method_returns_last_in_that_order() {
    let json = serde_json::to_string(&frag_def(
        &["GetAsync"],
        &["Prefix"],
        &["_log"],
        &[("GetAsync", "Task")],
        &[],
    ))
    .unwrap();
    assert_eq!(
        json,
        r#"{"id":"App.Facts.Widget","name":"Widget","namespace":"App.Facts","kind":"class","line":3,"methods":["GetAsync"],"properties":["Prefix"],"fields":["_log"],"methodReturns":{"GetAsync":"Task"}}"#
    );
}

#[test]
fn frag_def_omits_all_three_new_keys_when_the_type_declares_none_of_them() {
    let json = serde_json::to_string(&frag_def(&["Go"], &[], &[], &[], &[])).unwrap();
    assert_eq!(
        json,
        r#"{"id":"App.Facts.Widget","name":"Widget","namespace":"App.Facts","kind":"class","line":3,"methods":["Go"]}"#,
        "pre-stage-2 bytes preserved exactly"
    );
}

// --- extensionMethods lands AFTER methodReturns, entry keys
// in (name, thisType, arityMin, arityMax, thisArgs) order, omitted when
// empty; `bases` lands after extensionMethods -------------------------

#[test]
fn frag_def_appends_extension_methods_after_method_returns_with_the_arity_range_last() {
    let json = serde_json::to_string(&frag_def(
        &["Go"],
        &["Prefix"],
        &["_log"],
        &[("Go", "Task")],
        &[
            ("Render", "Widget", 0, 0),
            ("Render", "Widget", 2, 3),
            ("Trim", "string", 0, -1),
        ],
    ))
    .unwrap();
    assert_eq!(
        json,
        r#"{"id":"App.Facts.Widget","name":"Widget","namespace":"App.Facts","kind":"class","line":3,"methods":["Go"],"properties":["Prefix"],"fields":["_log"],"methodReturns":{"Go":"Task"},"extensionMethods":[{"name":"Render","thisType":"Widget","arityMin":0,"arityMax":0},{"name":"Render","thisType":"Widget","arityMin":2,"arityMax":3},{"name":"Trim","thisType":"string","arityMin":0,"arityMax":-1}]}"#
    );
}

#[test]
fn frag_def_this_args_lands_after_arity_max_and_bases_after_extension_methods() {
    let mut d = frag_def_with_bases(
        &["Go"],
        &[],
        &[],
        &[],
        &[("Each", "List", 0, 0)],
        &["BaseWidget", "IWidget"],
    );
    d.extension_methods[0].this_args = Some(vec!["Widget".into()]);
    let json = serde_json::to_string(&d).unwrap();
    assert_eq!(
        json,
        r#"{"id":"App.Facts.Widget","name":"Widget","namespace":"App.Facts","kind":"class","line":3,"methods":["Go"],"extensionMethods":[{"name":"Each","thisType":"List","arityMin":0,"arityMax":0,"thisArgs":["Widget"]}],"bases":["BaseWidget","IWidget"]}"#
    );
    let reparsed: FragDef = serde_json::from_str(&json).unwrap();
    assert_eq!(
        reparsed.bases,
        vec!["BaseWidget".to_string(), "IWidget".to_string()]
    );
    assert_eq!(
        reparsed.extension_methods[0].this_args,
        Some(vec!["Widget".to_string()])
    );
}

#[test]
fn frag_def_omits_bases_when_the_type_lists_none() {
    let json = serde_json::to_string(&frag_def(&["Go"], &[], &[], &[], &[])).unwrap();
    assert!(
        !json.contains("bases"),
        "an empty base list must be omitted entirely: {json}"
    );
    let reparsed: FragDef = serde_json::from_str(&json).unwrap();
    assert!(
        reparsed.bases.is_empty(),
        "an absent key reads back as \"lists none\""
    );
}

// --- test-coverage stage: testMethods lands AFTER bases, omitted when
// the type declares no tests ------------------------------------------

#[test]
fn frag_def_appends_test_methods_last_after_bases() {
    let mut d = frag_def_with_bases(&["Go"], &[], &[], &[], &[], &["BaseWidget"]);
    d.test_methods = vec!["TotalsAnEmptyOrder".into(), "TotalsALineItem".into()];
    let json = serde_json::to_string(&d).unwrap();
    assert_eq!(
        json,
        r#"{"id":"App.Facts.Widget","name":"Widget","namespace":"App.Facts","kind":"class","line":3,"methods":["Go"],"bases":["BaseWidget"],"testMethods":["TotalsAnEmptyOrder","TotalsALineItem"]}"#
    );
    let reparsed: FragDef = serde_json::from_str(&json).unwrap();
    assert_eq!(
        reparsed.test_methods,
        vec![
            "TotalsAnEmptyOrder".to_string(),
            "TotalsALineItem".to_string()
        ]
    );
}

#[test]
fn frag_def_omits_test_methods_when_the_type_declares_no_tests() {
    let json = serde_json::to_string(&frag_def(&["Go"], &[], &[], &[], &[])).unwrap();
    assert!(
        !json.contains("testMethods"),
        "pre-stage bytes preserved exactly: {json}"
    );
    let reparsed: FragDef = serde_json::from_str(&json).unwrap();
    assert!(
        reparsed.test_methods.is_empty(),
        "an absent key reads back as \"declares no tests\""
    );
}

#[test]
fn frag_def_omits_extension_methods_when_the_type_declares_none() {
    let json = serde_json::to_string(&frag_def(&["Go"], &[], &[], &[("Go", "Task")], &[])).unwrap();
    assert!(
        !json.contains("extensionMethods"),
        "pre-stage-3 bytes preserved exactly: {json}"
    );
    let reparsed: FragDef = serde_json::from_str(&json).unwrap();
    assert!(
        reparsed.extension_methods.is_empty(),
        "an absent key reads back as \"declares none\""
    );
}

#[test]
fn frag_def_method_returns_serializes_in_first_declaration_order_not_sorted() {
    // A BTreeMap here would emit Alpha before Zebra -- the required order
    // is insertion (first-declaration) order.
    let json = serde_json::to_string(&frag_def(
        &[],
        &[],
        &[],
        &[("Zebra", "Z"), ("Alpha", "A")],
        &[],
    ))
    .unwrap();
    assert!(
        json.ends_with(r#""methodReturns":{"Zebra":"Z","Alpha":"A"}}"#),
        "insertion order, not sorted: {json}"
    );
    let reparsed: FragDef = serde_json::from_str(&json).unwrap();
    let keys: Vec<&String> = reparsed.method_returns.iter().map(|(k, _)| k).collect();
    assert_eq!(
        keys,
        vec!["Zebra", "Alpha"],
        "and the order survives a round trip"
    );
}

// --- propertyTypes lands AFTER testMethods, omitted when the
// type declares no typed property ---------------------------------------

#[test]
fn frag_def_appends_property_types_last_after_test_methods() {
    let mut d = frag_def_with_bases(&["Go"], &["Dial", "Slots"], &[], &[], &[], &["BaseWidget"]);
    d.test_methods = vec!["TotalsALineItem".into()];
    d.property_types.insert(
        "Dial".into(),
        FragFact {
            type_name: "Gauge".into(),
            args: None,
        },
    );
    d.property_types.insert(
        "Slots".into(),
        FragFact {
            type_name: "Toolbox".into(),
            args: Some(vec!["Gadget".into()]),
        },
    );
    let json = serde_json::to_string(&d).unwrap();
    assert_eq!(
        json,
        r#"{"id":"App.Facts.Widget","name":"Widget","namespace":"App.Facts","kind":"class","line":3,"methods":["Go"],"properties":["Dial","Slots"],"bases":["BaseWidget"],"testMethods":["TotalsALineItem"],"propertyTypes":{"Dial":{"type":"Gauge"},"Slots":{"type":"Toolbox","args":["Gadget"]}}}"#
    );
    let reparsed: FragDef = serde_json::from_str(&json).unwrap();
    let keys: Vec<&String> = reparsed.property_types.iter().map(|(k, _)| k).collect();
    assert_eq!(
        keys,
        vec!["Dial", "Slots"],
        "source order survives a round trip, like methodReturns"
    );
}

#[test]
fn frag_def_omits_property_types_when_no_property_carries_a_fact() {
    let json = serde_json::to_string(&frag_def(&["Go"], &["Label"], &[], &[], &[])).unwrap();
    assert!(
        !json.contains("propertyTypes"),
        "pre-propertyTypes bytes preserved exactly: {json}"
    );
    let reparsed: FragDef = serde_json::from_str(&json).unwrap();
    assert!(
        reparsed.property_types.is_empty(),
        "an absent key reads back as \"no property vouches for a type\""
    );
}

// --- FragRef's receiverType then argCount, appended
// after generic in that order ------------------------------------------

fn frag_ref(generic: bool, receiver_type: Option<&str>, arg_count: Option<usize>) -> FragRef {
    FragRef {
        kind: "uses-member".into(),
        name: "repo".into(),
        qualified: None,
        member: Some("Save".into()),
        line: 7,
        namespace: Some("App.Shape".into()),
        type_arg_count: None,
        generic,
        receiver_type: receiver_type.map(String::from),
        arg_count,
        receiver_args: None,
        outer_types: Vec::new(),
        args: None,
        receiver_property_owner: None,
        receiver_call_owner: None,
        receiver_call_member: None,
        receiver_base: false,
        receiver_awaited: false,
        receiver_local: false,
        receiver_lambda: None,
    }
}

#[test]
fn frag_ref_receiver_args_is_appended_last_after_arg_count() {
    let r = FragRef {
        receiver_args: Some(vec!["FutureState".into(), "*".into()]),
        ..frag_ref(false, Some("Binder"), Some(1))
    };
    let json = serde_json::to_string(&r).unwrap();
    assert_eq!(
        json,
        r#"{"kind":"uses-member","name":"repo","member":"Save","line":7,"namespace":"App.Shape","receiverType":"Binder","argCount":1,"receiverArgs":["FutureState","*"]}"#
    );
    let reparsed: FragRef = serde_json::from_str(&json).unwrap();
    assert_eq!(
        reparsed.receiver_args,
        Some(vec!["FutureState".to_string(), "*".to_string()])
    );
}

#[test]
fn frag_ref_receiver_type_then_arg_count_are_appended_last_after_generic() {
    let json = serde_json::to_string(&frag_ref(true, Some("IRepo"), Some(2))).unwrap();
    assert_eq!(
        json,
        r#"{"kind":"uses-member","name":"repo","member":"Save","line":7,"namespace":"App.Shape","generic":true,"receiverType":"IRepo","argCount":2}"#
    );
}

#[test]
fn frag_ref_arg_count_zero_still_serializes_it() {
    // The guard is presence (argCount may be 0), never truthiness -- a
    // zero-argument call is a real call and its 0 is what matches an
    // arity-0 extension.
    let json = serde_json::to_string(&frag_ref(false, Some("IRepo"), Some(0))).unwrap();
    assert!(
        json.ends_with(r#""receiverType":"IRepo","argCount":0}"#),
        "argCount 0 must survive: {json}"
    );
}

#[test]
fn frag_ref_without_a_fact_keeps_its_pre_stage_2_bytes() {
    let json = serde_json::to_string(&frag_ref(false, None, None)).unwrap();
    assert_eq!(
        json,
        r#"{"kind":"uses-member","name":"repo","member":"Save","line":7,"namespace":"App.Shape"}"#
    );
    let reparsed: FragRef = serde_json::from_str(&json).unwrap();
    assert_eq!(
        reparsed.receiver_type, None,
        "an absent key reads back as no fact"
    );
    assert_eq!(
        reparsed.arg_count, None,
        "and an absent argCount reads back as \"not a call\""
    );
    assert_eq!(
        reparsed.receiver_args, None,
        "and an absent receiverArgs reads back as \"not generic\""
    );
}

// --- The cache holds two fragment shapes -------------------

// The untagged discrimination is what keeps one cache file able to hold
// both shapes: read the wrong arm and a whole repo's fragments come back
// silently empty. Both directions are pinned here, on the exact
// serialized bytes.
#[test]
fn any_fragment_round_trips_both_shapes_and_never_reads_one_as_the_other() {
    let cs = AnyFragment::Cs(Fragment {
        defs: Vec::new(),
        usings: Vec::new(),
        refs: Vec::new(),
        names: Vec::new(),
        registrations: Vec::new(),
    });
    let cs_json = serde_json::to_string(&cs).unwrap();
    assert_eq!(cs_json, r#"{"defs":[],"usings":[],"refs":[],"names":[]}"#);
    assert!(matches!(
        serde_json::from_str::<AnyFragment>(&cs_json).unwrap(),
        AnyFragment::Cs(_)
    ));

    let ts = AnyFragment::Ts(extract::TsFragment {
        ts: 1,
        defs: vec![extract::TsFragmentDef {
            name: "x".into(),
            kind: "const".into(),
            line: 1,
            end_line: 1,
        }],
        imports: Vec::new(),
        reexports: Vec::new(),
        refs: Vec::new(),
        default: None,
    });
    let ts_json = serde_json::to_string(&ts).unwrap();
    assert_eq!(
        ts_json,
        r#"{"ts":1,"defs":[{"name":"x","kind":"const","line":1,"endLine":1}],"imports":[],"reexports":[],"refs":[]}"#
    );
    assert!(matches!(
        serde_json::from_str::<AnyFragment>(&ts_json).unwrap(),
        AnyFragment::Ts(_)
    ));
    assert_eq!(
        serde_json::from_str::<AnyFragment>(&ts_json).unwrap(),
        ts,
        "and round-trips to the same value"
    );
}

// --- v8: FragRef's outerTypes, appended last -----------------------------

#[test]
fn frag_ref_outer_types_is_appended_last_after_receiver_args() {
    let r = FragRef {
        receiver_args: Some(vec!["FutureState".into()]),
        outer_types: vec!["Outer".into(), "Inner".into()],
        ..frag_ref(false, Some("Binder"), Some(1))
    };
    let json = serde_json::to_string(&r).unwrap();
    assert_eq!(
        json,
        r#"{"kind":"uses-member","name":"repo","member":"Save","line":7,"namespace":"App.Shape","receiverType":"Binder","argCount":1,"receiverArgs":["FutureState"],"outerTypes":["Outer","Inner"]}"#
    );
    let reparsed: FragRef = serde_json::from_str(&json).unwrap();
    assert_eq!(
        reparsed.outer_types,
        vec!["Outer".to_string(), "Inner".to_string()]
    );
}

// --- The two chain facts, appended after outerTypes ----

#[test]
fn frag_ref_property_owner_and_call_pair_are_appended_last_and_omitted_when_absent() {
    let hop = FragRef {
        qualified: Some("head.Dial".into()),
        name: "Dial".into(),
        receiver_property_owner: Some("Widget".into()),
        ..frag_ref(false, None, Some(0))
    };
    assert_eq!(
        serde_json::to_string(&hop).unwrap(),
        r#"{"kind":"uses-member","name":"Dial","qualified":"head.Dial","member":"Save","line":7,"namespace":"App.Shape","argCount":0,"receiverPropertyOwner":"Widget"}"#
    );

    let from_call = FragRef {
        receiver_call_owner: Some("Factory".into()),
        receiver_call_member: Some("Make".into()),
        ..frag_ref(false, None, Some(0))
    };
    assert_eq!(
        serde_json::to_string(&from_call).unwrap(),
        r#"{"kind":"uses-member","name":"repo","member":"Save","line":7,"namespace":"App.Shape","argCount":0,"receiverCallOwner":"Factory","receiverCallMember":"Make"}"#
    );

    // An older cached fragment carries neither key, and absent must
    // read back as "no chain fact" -- which is what leaves such a ref
    // exactly where it was before this generation.
    let plain = serde_json::to_string(&frag_ref(false, Some("Binder"), Some(1))).unwrap();
    assert!(
        !plain.contains("receiverPropertyOwner") && !plain.contains("receiverCall"),
        "{plain}"
    );
    let reparsed: FragRef = serde_json::from_str(&plain).unwrap();
    assert_eq!(reparsed.receiver_property_owner, None);
    assert_eq!(reparsed.receiver_call_owner, None);
    assert_eq!(reparsed.receiver_call_member, None);
}

#[test]
fn frag_ref_an_empty_or_absent_outer_types_serializes_to_nothing_and_reads_back_empty() {
    let json = serde_json::to_string(&frag_ref(false, None, None)).unwrap();
    assert!(
        !json.contains("outerTypes"),
        "a namespace-level ref keeps its exact pre-v8 bytes: {json}"
    );
    // A pre-v8 cached fragment has no key at all, and absent must mean the
    // same thing as empty -- which is what keeps it off the nested step.
    let pre_v8: FragRef = serde_json::from_str(
        r#"{"kind":"uses-type","name":"Widget","line":3,"namespace":"App.Core"}"#,
    )
    .unwrap();
    assert!(pre_v8.outer_types.is_empty());
}

// --- v18: FragDef's methodParams and FragRef's receiverLambda -----------

#[test]
fn fragment_round_trip_keeps_method_params_and_receiver_lambda() {
    let mut method_params = OrderedMap::new();
    method_params.insert(
        "Register".to_string(),
        vec![
            vec!["Action<Options>".to_string()],
            vec!["string".to_string(), "Func<Options,bool>".to_string()],
        ],
    );
    let d = FragDef {
        method_params: method_params.clone(),
        ..frag_def(&[], &[], &[], &[], &[])
    };
    let d_json = serde_json::to_string(&d).unwrap();
    assert!(
        d_json.ends_with(
            r#""methodParams":{"Register":[["Action<Options>"],["string","Func<Options,bool>"]]}}"#
        ),
        "methodParams is appended last, after methodArities: {d_json}"
    );
    let d_reparsed: FragDef = serde_json::from_str(&d_json).unwrap();
    assert_eq!(d_reparsed.method_params, method_params);

    // Absent when empty, and an absent key deserializes back to empty --
    // the safe default for every fragment cached before this field
    // existed.
    let d_plain_json = serde_json::to_string(&frag_def(&[], &[], &[], &[], &[])).unwrap();
    assert!(!d_plain_json.contains("methodParams"), "{d_plain_json}");
    let d_plain_reparsed: FragDef = serde_json::from_str(&d_plain_json).unwrap();
    assert!(d_plain_reparsed.method_params.is_empty());

    let slot = FragLambdaSlot {
        owner: "Registrar".to_string(),
        member: "Register".to_string(),
        arg_count: 1,
        arg_index: 0,
        arity: 1,
        index: 0,
    };
    let r = FragRef {
        receiver_lambda: Some(slot.clone()),
        ..frag_ref(false, None, None)
    };
    let r_json = serde_json::to_string(&r).unwrap();
    assert!(
        r_json.ends_with(
            r#""receiverLambda":{"owner":"Registrar","member":"Register","argCount":1,"argIndex":0,"arity":1,"index":0}}"#
        ),
        "receiverLambda is appended last, after receiverLocal: {r_json}"
    );
    let r_reparsed: FragRef = serde_json::from_str(&r_json).unwrap();
    assert_eq!(r_reparsed.receiver_lambda, Some(slot));

    // Absent when `None`, and an absent key deserializes back to `None`.
    let r_plain_json = serde_json::to_string(&frag_ref(false, None, None)).unwrap();
    assert!(!r_plain_json.contains("receiverLambda"), "{r_plain_json}");
    let r_plain_reparsed: FragRef = serde_json::from_str(&r_plain_json).unwrap();
    assert_eq!(r_plain_reparsed.receiver_lambda, None);
}
