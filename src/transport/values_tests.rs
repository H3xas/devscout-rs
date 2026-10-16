use super::*;

fn ok(text: &str, val: Val) -> Parsed {
    classify(text, val).unwrap_or_else(|r| panic!("{text}: {r}"))
}

#[test]
fn transport_facts_ac_4_literal_uri_forms_keep_their_scheme() {
    for (text, scheme, value) in [
        ("queue:submit-order", "queue", "submit-order"),
        (
            "exchange:check-stock?type=fanout",
            "exchange",
            "check-stock",
        ),
        ("topic:order-events", "topic", "order-events"),
    ] {
        let parsed = ok(text, Val::Address);
        assert_eq!(parsed.scheme.as_deref(), Some(scheme), "{text}");
        assert_eq!(parsed.value.as_deref(), Some(value), "{text}");
    }
    let broker = ok(
        "rabbitmq://broker.internal:5672/ledger/invoices",
        Val::Address,
    );
    assert_eq!(broker.host.as_deref(), Some("broker.internal"));
    assert_eq!(broker.virtual_host.as_deref(), Some("ledger"));
    assert_eq!(broker.value.as_deref(), Some("invoices"));
    let sqs = ok("amazonsqs://eu-west-1/scope/archive", Val::Address);
    assert_eq!(sqs.region.as_deref(), Some("eu-west-1"));
    assert_eq!(sqs.value.as_deref(), Some("archive"));
}

#[test]
fn transport_facts_ac_4_unreadable_strings_name_their_reason() {
    assert_eq!(classify("amqp://broker/x", Val::Address), Err("uri-scheme"));
    assert_eq!(classify("orders", Val::Address), Err("malformed-uri"));
    assert_eq!(
        classify("rabbitmq://broker.internal/", Val::Address),
        Err("malformed-uri")
    );
    assert_eq!(classify("bad name", Val::Name), Err("unread-value"));
    assert_eq!(
        classify("amazonsqs://not_a_region/q", Val::Address),
        Err("unread-value")
    );
    assert_eq!(classify("Region-1", Val::Region), Err("unread-value"));
    assert_eq!(classify("amqp://u:p@h", Val::HostName), Err("uri-scheme"));
}

#[test]
fn transport_facts_ac_5_resource_names_and_host_uris_state_their_scope() {
    let arn = ok(
        "arn:aws:sns:us-east-2:123456789012:order-submitted",
        Val::EntityName,
    );
    assert_eq!(arn.region.as_deref(), Some("us-east-2"));
    assert_eq!(arn.account.as_deref(), Some("123456789012"));
    assert_eq!(arn.value.as_deref(), Some("order-submitted"));
    assert_eq!(
        classify("arn:aws:sns:us-east-2:12345:x", Val::EntityName),
        Err("unread-value")
    );
    let host = ok("rabbitmq://broker.internal/ledger", Val::HostUri);
    assert_eq!(host.host.as_deref(), Some("broker.internal"));
    assert_eq!(host.virtual_host.as_deref(), Some("ledger"));
    let bare = ok("rabbitmq://broker.internal", Val::HostUri);
    assert_eq!(
        bare.virtual_host, None,
        "a URI with no path states no virtual host"
    );
}

#[test]
fn transport_facts_ac_6_user_information_never_survives_a_uri() {
    let parsed = ok(
        "rabbitmq://ops:s3cret@broker.internal/ledger/q",
        Val::Address,
    );
    let text = serde_json::to_string(&parsed).expect("serializes");
    assert!(!text.contains("s3cret") && !text.contains("ops"), "{text}");
    assert_eq!(
        classify("ops:s3cret@broker", Val::HostName),
        Err("unread-value")
    );
}
