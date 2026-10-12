use super::*;

#[test]
fn argument_types_round_trip_in_source_order_and_omit_on_reads() {
    let call = FragRef {
        arg_types: Some(vec![
            Some(FragArgType::Known("string".into())),
            None,
            Some(FragArgType::Member {
                owner: "TimeSpan".into(),
                member: "FromSeconds".into(),
                receiver_args: None,
                call: true,
            }),
        ]),
        ..frag_ref(false, Some("IRepo"), Some(3))
    };
    let json = serde_json::to_string(&call).unwrap();
    assert!(
        json.ends_with(
            r#""argTypes":["string",null,{"owner":"TimeSpan","member":"FromSeconds","call":true}]}"#
        ),
        "{json}"
    );
    let parsed: FragRef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.arg_types, call.arg_types);
    assert!(!serde_json::to_string(&frag_ref(false, None, None))
        .unwrap()
        .contains("argTypes"));
}
