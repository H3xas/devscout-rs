//! Acceptance tests for the MassTransit transport facts over the
//! `fixtures/transport-masstransit` repository. Each test reads the facts
//! `graph.json` records, or the `transport-address` lines the export writes.

#[path = "fact_export_support/mod.rs"]
mod support;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::Value;
use support::{family_lines, trailer_of, Fixture};

fn source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/transport-masstransit")
}

fn shared() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| Fixture::build_from(&source(), "transport"))
}

fn facts(fixture: &Fixture) -> Vec<Value> {
    fixture.graph()["transport"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn of_kind<'f>(facts: &'f [Value], kind: &str) -> Vec<&'f Value> {
    facts.iter().filter(|f| f["kind"] == kind).collect()
}

fn in_file<'f>(facts: &'f [Value], file: &str, kind: &str) -> Vec<&'f Value> {
    facts
        .iter()
        .filter(|f| f["file"] == file && f["kind"] == kind)
        .collect()
}

fn addressed<'f>(facts: &'f [Value], kind: &str, value: &str) -> Vec<&'f Value> {
    facts
        .iter()
        .filter(|f| f["kind"] == kind && f["address"]["value"] == value)
        .collect()
}

fn reasons(facts: &[Value]) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for fact in facts {
        collect_reasons(fact, &mut found);
    }
    found
}

fn collect_reasons(value: &Value, found: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if map.get("class") == Some(&Value::from("unknown")) {
                if let Some(reason) = map.get("reason").and_then(Value::as_str) {
                    found.insert(reason.to_string());
                }
            }
            map.values().for_each(|v| collect_reasons(v, found));
        }
        Value::Array(items) => items.iter().for_each(|v| collect_reasons(v, found)),
        _ => {}
    }
}

fn scope_part<'v>(fact: &'v Value, part: &str) -> &'v Value {
    &fact["scope"][part]
}

#[test]
fn transport_facts_ac_1_no_other_production_file_spells_a_catalogue_name() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let catalogue = src.join("transport/catalogue.rs");
    let catalogue_dir = src.join("transport/catalogue");
    let mut offenders = Vec::new();
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("read src").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let is_test = name.ends_with("_tests.rs")
                || name == "tests.rs"
                || path.components().any(|c| c.as_os_str() == "tests");
            if path == catalogue
                || path.starts_with(&catalogue_dir)
                || is_test
                || !name.ends_with(".rs")
            {
                continue;
            }
            let text = fs::read_to_string(&path).expect("read source");
            for spelled in ["\"MassTransit.", "\"Microsoft.Extensions.", "\"System.Uri"] {
                if text.contains(spelled) {
                    offenders.push(format!("{}: {spelled}", path.display()));
                }
            }
        }
    }
    assert!(offenders.is_empty(), "{offenders:#?}");
}

#[test]
fn transport_facts_ac_2_every_receiver_shape_gives_its_fact() {
    let facts = facts(shared());
    let program = "src/Service/Program.cs";
    assert_eq!(
        in_file(&facts, program, "bus").len(),
        1,
        "dotted chain start"
    );
    assert_eq!(
        addressed(&facts, "receive-endpoint", "order-intake").len(),
        3,
        "nested lambdas, a `Using` item and a global using file"
    );
    assert_eq!(
        in_file(&facts, program, "options-binding").len(),
        1,
        "the SDK's implicit usings import the options extension methods"
    );
    assert_eq!(
        in_file(&facts, program, "endpoint-convention").len(),
        2,
        "the static entry point"
    );
    assert_eq!(
        in_file(&facts, "src/Billing/BillingBus.cs", "bus")[0]["member"],
        "MassTransit.RabbitMqBusFactoryConfiguratorExtensions.CreateUsingRabbitMq",
        "the `Bus.Factory` entry point"
    );
    let sends = in_file(&facts, "src/Billing/BillingBus.cs", "send-address");
    assert!(
        sends.len() >= 3,
        "a field, a property and a typed local receiver: {sends:#?}"
    );
    assert!(
        !in_file(&facts, "src/Billing/Values.cs", "receive-endpoint").is_empty(),
        "a method parameter receiver"
    );
    assert_eq!(
        in_file(&facts, "src/Service/Consumers/Consumers.cs", "send-address").len(),
        1,
        "a consume-context parameter receiver"
    );
}

#[test]
fn transport_facts_ac_2_a_this_qualified_member_keeps_its_declared_type() {
    let facts = facts(shared());
    for value in [
        "this-field-refunds",
        "this-property-refunds",
        "this-shadowed-refunds",
    ] {
        assert_eq!(addressed(&facts, "send-address", value).len(), 1, "{value}");
    }
    let sends = in_file(&facts, "src/Billing/ThisQualified.cs", "send-address");
    assert_eq!(sends.len(), 4);
    assert_eq!(sends[3]["address"]["reason"], "field");
}

#[test]
fn transport_facts_ac_2_a_lookalike_type_out_of_scope_refuses_nothing() {
    let facts = facts(shared());
    let buses: Vec<&str> = in_file(&facts, "src/Worker/WorkerBus.cs", "bus")
        .iter()
        .filter_map(|f| f["transport"].as_str())
        .collect();
    assert!(buses.contains(&"in-memory"), "{buses:?}");
}

#[test]
fn transport_facts_ac_2_each_refusal_gives_no_fact() {
    let facts = facts(shared());
    let refused: Vec<&Value> = facts
        .iter()
        .filter(|f| {
            f["file"]
                .as_str()
                .is_some_and(|p| p.starts_with("src/Refusals/"))
        })
        .filter(|f| f["kind"] != "options-binding")
        .collect();
    assert!(refused.is_empty(), "{refused:#?}");
    let named: Vec<&Value> = facts
        .iter()
        .filter(|f| {
            f["address"]["value"]
                .as_str()
                .is_some_and(|v| v.starts_with("refused-"))
        })
        .collect();
    assert!(named.is_empty(), "{named:#?}");
}

#[test]
fn transport_facts_ac_2_the_readme_has_a_row_for_every_source_file() {
    let readme = fs::read_to_string(source().join("README.md")).expect("read README");
    let mut stack = vec![source()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("read fixture").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(source())
                .expect("inside the fixture")
                .to_string_lossy()
                .replace('\\', "/");
            if rel == "README.md" || rel.ends_with(".json") || rel.ends_with(".csproj") {
                continue;
            }
            assert!(
                readme.contains(&format!("`{rel}`")),
                "README.md has no row naming {rel}"
            );
        }
    }
}

#[test]
fn transport_facts_ac_3_every_kind_is_recorded() {
    let facts = facts(shared());
    for kind in [
        "bus",
        "host",
        "receive-endpoint",
        "configure-endpoints",
        "registration",
        "endpoint-name",
        "send-address",
        "request-address",
        "endpoint-convention",
        "entity-name",
        "options-binding",
        "outbox",
    ] {
        assert!(!of_kind(&facts, kind).is_empty(), "no {kind} fact");
    }
    let words: BTreeSet<&str> = of_kind(&facts, "registration")
        .iter()
        .filter_map(|f| f["registration"].as_str())
        .collect();
    for word in ["consumer", "state-machine", "consumer-namespace"] {
        assert!(words.contains(word), "{word}: {words:?}");
    }
}

#[test]
fn transport_facts_ac_3_nested_facts_name_their_bus() {
    let facts = facts(shared());
    let program = "src/Service/Program.cs";
    let bus_line = in_file(&facts, program, "bus")[0]["line"].clone();
    for kind in [
        "registration",
        "host",
        "receive-endpoint",
        "configure-endpoints",
        "outbox",
    ] {
        for fact in in_file(&facts, program, kind) {
            assert_eq!(fact["busFile"], program, "{kind}: {fact}");
            assert_eq!(fact["busLine"], bus_line, "{kind}: {fact}");
        }
    }
    let unlinked = in_file(&facts, "src/Billing/Values.cs", "receive-endpoint");
    assert!(unlinked.iter().all(|f| f.get("busFile").is_none()));
}

#[test]
fn transport_facts_ac_3_a_receive_endpoint_holds_its_configured_types() {
    let facts = facts(shared());
    let intake = in_file(&facts, "src/Service/Program.cs", "receive-endpoint")
        .into_iter()
        .find(|f| f["address"]["value"] == "order-intake")
        .expect("the intake endpoint");
    let types: Vec<(String, String)> = intake["types"]
        .as_array()
        .expect("types")
        .iter()
        .map(|t| {
            assert!(t["line"].as_u64().is_some(), "each item has its line: {t}");
            (t["role"].to_string(), t["type"].to_string())
        })
        .collect();
    assert_eq!(
        types,
        vec![
            (
                "\"consumer\"".to_string(),
                "\"SubmitOrderConsumer\"".to_string()
            ),
            ("\"handler\"".to_string(), "\"OrderNoted\"".to_string()),
        ]
    );
}

#[test]
fn transport_facts_ac_3_the_formatter_in_force_is_stated_not_applied() {
    let facts = facts(shared());
    let words: Vec<(String, String)> = of_kind(&facts, "configure-endpoints")
        .iter()
        .map(|f| (f["formatter"].to_string(), f["formatterSource"].to_string()))
        .collect();
    for expected in [
        ("\"kebab-case\"", "\"bus\""),
        ("\"snake-case\"", "\"argument\""),
        ("\"unknown\"", "\"argument\""),
        ("\"default\"", "\"default\""),
    ] {
        assert!(
            words.contains(&(expected.0.to_string(), expected.1.to_string())),
            "{expected:?} in {words:?}"
        );
    }
    let snake = of_kind(&facts, "configure-endpoints")
        .into_iter()
        .find(|f| f["formatter"] == "snake-case")
        .expect("the snake-case call");
    assert_eq!(snake["formatterArguments"][0]["value"], "billing");
    assert_eq!(snake["formatterArguments"][1]["value"], "false");
    assert!(of_kind(&facts, "configure-endpoints")
        .iter()
        .any(|f| f["filtered"] == true));
    for fact in of_kind(&facts, "configure-endpoints") {
        assert!(
            fact.get("address").is_none(),
            "no derived queue name: {fact}"
        );
    }
}

#[test]
fn transport_facts_ac_3_no_address_value_is_absent_from_the_source() {
    let facts = facts(shared());
    let mut corpus = String::new();
    let mut stack = vec![source()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("read fixture").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                corpus.push_str(&fs::read_to_string(&path).unwrap_or_default());
            }
        }
    }
    let mut values = Vec::new();
    for fact in &facts {
        collect_values(fact, &mut values);
    }
    for value in values {
        let stated = corpus.contains(&value)
            || value
                .split(['-', '.'])
                .all(|part| part.is_empty() || corpus.contains(part));
        assert!(stated, "`{value}` appears nowhere in the fixture");
    }
}

fn collect_values(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(v) = map.get("value").and_then(Value::as_str) {
                out.push(v.to_string());
            }
            map.values().for_each(|v| collect_values(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| collect_values(v, out)),
        _ => {}
    }
}

#[test]
fn transport_facts_ac_4_each_class_reads_as_documented() {
    let facts = facts(shared());
    let convention = addressed(&facts, "endpoint-convention", "submit-order");
    assert_eq!(convention[0]["address"]["class"], "literal");
    assert_eq!(convention[0]["address"]["scheme"], "queue");
    for (kind, value, scheme) in [
        ("request-address", "check-stock", "exchange"),
        ("endpoint-convention", "shipment-events", "topic"),
        ("send-address", "invoices", "rabbitmq"),
        ("send-address", "order-archive", "amazonsqs"),
    ] {
        let found = addressed(&facts, kind, value);
        assert_eq!(found.len(), 1, "{kind} {value}");
        assert_eq!(found[0]["address"]["scheme"], scheme, "{kind} {value}");
    }

    let audit = addressed(&facts, "receive-endpoint", "ledger-audit");
    assert_eq!(audit[0]["address"]["class"], "constant");
    let fields: Vec<&str> = audit[0]["address"]["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .filter_map(|f| f["field"].as_str())
        .collect();
    assert_eq!(
        fields,
        vec![
            "Fixture.Contracts.QueueNames.Audit",
            "Fixture.Contracts.QueueNames.Prefix"
        ],
        "a constant of a constant, in another file, in reading order"
    );
    let entity = in_file(&facts, "src/Service/Program.cs", "entity-name");
    assert_eq!(entity[0]["address"]["class"], "constant");
    assert_eq!(entity[0]["address"]["value"], "ledger-order-submitted");

    let binding = in_file(&facts, "src/Service/Program.cs", "options-binding");
    let host = &binding[0]["properties"][0]["value"];
    assert_eq!(host["class"], "configuration");
    assert_eq!(host["key"], "Broker:Host");
    assert_eq!(
        host["values"][0]["file"],
        "src/Service/appsettings.Development.json"
    );
    assert_eq!(host["values"][0]["value"], "localhost");
    assert_eq!(host["values"][1]["file"], "src/Service/appsettings.json");
    assert_eq!(host["values"][1]["value"], "broker.internal");

    let unbound = in_file(&facts, "src/Refusals/Unbound.cs", "options-binding");
    assert_eq!(
        unbound[0]["properties"][0]["value"]["values"],
        Value::Array(Vec::new()),
        "no settings file states the key"
    );
}

#[test]
fn transport_facts_ac_4_every_reason_word_occurs() {
    let found = reasons(&facts(shared()));
    for reason in [
        "interpolated-string",
        "parameter",
        "local",
        "static-readonly",
        "field",
        "method-call",
        "nameof",
        "expression",
        "package-constant",
        "unresolved-constant",
        "uri-scheme",
        "malformed-uri",
        "unread-value",
        "unreadable-configuration",
        "overload",
        "multiple-hosts",
    ] {
        assert!(found.contains(reason), "{reason}: {found:?}");
    }
    let facts = facts(shared());
    let values = in_file(&facts, "src/Billing/Values.cs", "receive-endpoint");
    let cycle = values
        .iter()
        .find(|f| f["line"] == 18)
        .expect("the cyclic constant");
    assert_eq!(cycle["address"]["reason"], "unresolved-constant");
    let rotating = values
        .iter()
        .find(|f| f["line"] == 12)
        .expect("static readonly");
    assert_eq!(rotating["address"]["reason"], "static-readonly");
}

fn rewrite(fixture: &Fixture, rel: &str, from: &str, to: &str) {
    let path = fixture.root.join(rel);
    let text = fs::read_to_string(&path).expect("read");
    assert!(text.contains(from), "{rel} holds {from}");
    fs::write(&path, text.replace(from, to)).expect("write");
}

#[test]
fn transport_facts_ac_4_a_settings_edit_alone_moves_the_value() {
    let fixture = Fixture::build_from(&source(), "transport-settings");
    rewrite(
        &fixture,
        "src/Service/appsettings.json",
        "\"broker.internal\"",
        "\"broker.edited\"",
    );
    fixture.map();
    let facts = facts(&fixture);
    let binding = in_file(&facts, "src/Service/Program.cs", "options-binding");
    assert_eq!(
        binding[0]["properties"][0]["value"]["values"][1]["value"],
        "broker.edited"
    );
}

#[test]
fn transport_facts_ac_4_an_implicit_usings_edit_alone_moves_the_binding() {
    let fixture = Fixture::build_from(&source(), "transport-usings");
    rewrite(
        &fixture,
        "src/Service/Service.csproj",
        "<ImplicitUsings>enable</ImplicitUsings>",
        "<ImplicitUsings>disable</ImplicitUsings>",
    );
    fixture.map();
    let facts = facts(&fixture);
    assert!(
        in_file(&facts, "src/Service/Program.cs", "options-binding").is_empty(),
        "without the SDK's usings nothing imports the options extension methods"
    );
}

#[test]
fn transport_facts_ac_5_one_queue_name_on_three_buses_exports_three_scopes() {
    let facts = facts(shared());
    let intake = addressed(&facts, "receive-endpoint", "order-intake");
    let scopes: BTreeSet<String> = intake.iter().map(|f| f["scope"].to_string()).collect();
    assert_eq!(scopes.len(), 3, "{scopes:#?}");
    let vhosts: BTreeSet<String> = intake
        .iter()
        .map(|f| scope_part(f, "virtualHost")["value"].to_string())
        .collect();
    assert!(vhosts.contains("\"ledger\"") && vhosts.contains("\"billing\""));
    let sqs = intake
        .iter()
        .find(|f| f["file"] == "src/Worker/WorkerBus.cs")
        .expect("the sqs endpoint");
    assert_eq!(scope_part(sqs, "region")["value"], "us-east-2");
    assert_eq!(scope_part(sqs, "host")["class"], "not-stated");
}

#[test]
fn transport_facts_ac_5_addresses_and_hosts_state_their_scope() {
    let facts = facts(shared());
    let arn = in_file(&facts, "src/Worker/WorkerBus.cs", "entity-name");
    assert_eq!(scope_part(arn[0], "account")["value"], "123456789012");
    assert_eq!(scope_part(arn[0], "region")["value"], "us-east-2");
    let archive = addressed(&facts, "send-address", "order-archive");
    assert_eq!(scope_part(archive[0], "region")["value"], "us-east-2");
    let credit = addressed(&facts, "request-address", "check-credit");
    assert_eq!(
        scope_part(credit[0], "host")["value"],
        "credit.internal",
        "the URI host over the bus host"
    );
    assert_eq!(scope_part(credit[0], "virtualHost")["value"], "credit");
    let hostless = addressed(&facts, "receive-endpoint", "hostless");
    assert_eq!(scope_part(hostless[0], "host")["class"], "not-stated");
    let twice = addressed(&facts, "receive-endpoint", "twice-hosted");
    assert_eq!(scope_part(twice[0], "host")["reason"], "multiple-hosts");
    let worker_bus = in_file(&facts, "src/Worker/WorkerBus.cs", "bus");
    assert_eq!(
        scope_part(worker_bus[0], "region")["value"],
        "us-east-2",
        "the options binding's region stays on the binding fact"
    );
}

#[test]
fn transport_facts_ac_5_a_named_argument_is_never_read_by_position() {
    let facts = facts(shared());
    let hosts = in_file(&facts, "src/Billing/NamedArguments.cs", "host");
    assert_eq!(hosts.len(), 2);
    for part in ["host", "virtualHost"] {
        assert_eq!(scope_part(hosts[0], part)["class"], "unknown", "{part}");
        assert_eq!(scope_part(hosts[0], part)["reason"], "overload", "{part}");
    }
    assert_eq!(scope_part(hosts[1], "host")["class"], "literal");
    assert_eq!(
        scope_part(hosts[1], "host")["value"],
        "ordered-broker.internal"
    );
    assert_eq!(scope_part(hosts[1], "virtualHost")["reason"], "overload");

    let reordered = addressed(&facts, "receive-endpoint", "named-intake");
    assert_eq!(scope_part(reordered[0], "host")["reason"], "overload");
    assert_eq!(
        scope_part(reordered[0], "virtualHost")["reason"],
        "overload"
    );

    let formatter = in_file(
        &facts,
        "src/Billing/NamedArguments.cs",
        "configure-endpoints",
    );
    assert_eq!(formatter.len(), 1);
    assert_eq!(formatter[0]["formatter"], "kebab-case");
    assert_eq!(formatter[0]["formatterArguments"][0]["value"], "ordered");
    assert_eq!(formatter[0]["formatterArguments"][1]["reason"], "overload");
}

#[test]
fn transport_facts_ac_6_no_secret_reaches_an_artifact_or_the_export() {
    let fixture = shared();
    let secrets = [
        "s3cret-broker-pass",
        "s3cret-db-pass",
        "s3cret-vhost-pass",
        "s3cret-config-amqp",
        "s3cret-config-pass",
        "s3cret-uri-pass",
        "s3cret-amqp-pass",
        "s3cret-joined-send",
        "s3cret-joined-middle",
        "s3cret-joined-host",
    ];
    let state = fixture.state_dir();
    let mut stack = vec![state];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("read state").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let bytes = fs::read(&path).expect("read artifact");
            let text = String::from_utf8_lossy(&bytes);
            for secret in secrets {
                assert!(!text.contains(secret), "{} holds {secret}", path.display());
            }
        }
    }
    let export = String::from_utf8(fixture.export(&[])).expect("utf-8 export");
    for secret in secrets {
        assert!(!export.contains(secret), "the export holds {secret}");
    }
}

#[test]
fn transport_facts_ac_6_user_information_joined_to_a_constant_still_resolves() {
    let facts = facts(shared());
    for (value, fields) in [
        (
            "joined-invoices",
            vec!["Fixture.Billing.BrokerParts.SendTail"],
        ),
        (
            "joined-refunds",
            vec![
                "Fixture.Billing.BrokerParts.Scheme",
                "Fixture.Billing.BrokerParts.User",
                "Fixture.Billing.BrokerParts.RefundTail",
            ],
        ),
    ] {
        let found = addressed(&facts, "send-address", value);
        assert_eq!(found.len(), 1, "{value}");
        let address = &found[0]["address"];
        assert_eq!(address["class"], "constant", "{value}");
        assert_eq!(address["scheme"], "rabbitmq", "{value}");
        let named: Vec<&str> = address["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .filter_map(|f| f["field"].as_str())
            .collect();
        assert_eq!(named, fields, "{value}");
        assert_eq!(scope_part(found[0], "host")["value"], "broker.internal");
        assert_eq!(scope_part(found[0], "virtualHost")["value"], "billing");
    }
    let host = in_file(&facts, "src/Billing/JoinedAddresses.cs", "host");
    assert_eq!(host.len(), 1);
    assert_eq!(scope_part(host[0], "host")["class"], "constant");
    assert_eq!(scope_part(host[0], "host")["value"], "broker.internal");
    assert_eq!(scope_part(host[0], "virtualHost")["value"], "joined");
}

const EARLIER_FORM_SECRET: &str = "s3cret-earlier-form";

// The cache pair as an earlier build of this generation wrote it: no payload
// stamp, and each joined literal cached as its text rather than its position.
fn rewrite_cache_in_earlier_form(fixture: &Fixture) {
    fn literal_text(value: &mut Value, replaced: &mut usize) {
        match value {
            Value::Object(map) => {
                if let Some(lit) = map.get_mut("lit") {
                    if lit.is_object() {
                        *lit = Value::String(format!("amqp://ops:{EARLIER_FORM_SECRET}@"));
                        *replaced += 1;
                        return;
                    }
                }
                map.values_mut().for_each(|v| literal_text(v, replaced));
            }
            Value::Array(items) => items.iter_mut().for_each(|v| literal_text(v, replaced)),
            _ => {}
        }
    }
    let cache = fixture.cache_path();
    let name = cache.file_name().and_then(|n| n.to_str()).expect("name");
    let index = cache.with_file_name(name.replacen("fragments-", "fragments-index-", 1));
    for path in [&cache, &index] {
        let mut stamped: Value =
            serde_json::from_str(&fs::read_to_string(path).expect("read")).expect("json");
        let map = stamped.as_object_mut().expect("a stamped object");
        map.retain(|key, _| key == "owner" || key == "entries");
        if path == &cache {
            let mut replaced = 0;
            literal_text(&mut stamped, &mut replaced);
            assert!(replaced > 0, "the fixture caches a joined literal");
        }
        fs::write(path, serde_json::to_vec(&stamped).expect("json")).expect("write");
    }
}

fn assert_no_earlier_form_secret(fixture: &Fixture) {
    let mut stack = vec![fixture.state_dir()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("read state").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let text = String::from_utf8_lossy(&fs::read(&path).expect("read")).into_owned();
                assert!(
                    !text.contains(EARLIER_FORM_SECRET),
                    "{} holds the earlier form's literal",
                    path.display()
                );
            }
        }
    }
}

fn graph_shape(fixture: &Fixture) -> (usize, usize, usize) {
    let graph = fixture.graph();
    let count = |key: &str| graph[key].as_array().map_or(0, Vec::len);
    (count("defs"), count("edges"), count("transport"))
}

#[test]
fn transport_facts_ac_6_a_cache_in_an_earlier_form_is_replaced_by_the_next_map() {
    let fixture = Fixture::build_from(&source(), "transport-earlier-form");
    let before = graph_shape(&fixture);
    assert!(before.2 > 0);
    rewrite_cache_in_earlier_form(&fixture);

    let refused = fixture.run(&["export"]);
    assert!(
        !refused.status.success(),
        "export must refuse a cache it cannot read in full"
    );

    fixture.map();
    assert_no_earlier_form_secret(&fixture);
    assert_eq!(graph_shape(&fixture), before);
    let export = String::from_utf8(fixture.export(&[])).expect("utf-8 export");
    assert!(!export.contains(EARLIER_FORM_SECRET));
}

#[test]
fn transport_facts_ac_6_a_settings_edit_over_an_earlier_form_keeps_the_graph() {
    let fixture = Fixture::build_from(&source(), "transport-earlier-form-settings");
    let before = graph_shape(&fixture);
    rewrite_cache_in_earlier_form(&fixture);
    rewrite(
        &fixture,
        "src/Service/appsettings.json",
        "\"broker.internal\"",
        "\"broker.edited\"",
    );

    fixture.map();
    assert_no_earlier_form_secret(&fixture);
    assert_eq!(graph_shape(&fixture), before);
    let facts = facts(&fixture);
    let binding = in_file(&facts, "src/Service/Program.cs", "options-binding");
    assert_eq!(
        binding[0]["properties"][0]["value"]["values"][1]["value"],
        "broker.edited"
    );
}

#[test]
fn transport_facts_ac_7_a_test_substitution_stays_a_test_fact() {
    let fixture = shared();
    let facts = facts(fixture);
    let harness = in_file(&facts, "tests/Fixture.Tests/OrderFlowTests.cs", "bus");
    assert_eq!(harness.len(), 1);
    assert_eq!(harness[0]["transport"], "test-harness");
    let production = in_file(&facts, "src/Service/Program.cs", "bus");
    assert_eq!(production[0]["transport"], "rabbitmq");
    let local = in_file(&facts, "src/Service/LocalHarness.cs", "bus");
    assert_eq!(local[0]["transport"], "test-harness");

    let lines = fixture.lines();
    let test_files: BTreeSet<String> = family_lines(&lines, "test-file")
        .iter()
        .filter_map(|f| f["file"].as_str().map(str::to_string))
        .collect();
    assert!(test_files.contains("tests/Fixture.Tests/OrderFlowTests.cs"));
    assert!(!test_files.contains("src/Service/LocalHarness.cs"));
}

#[test]
fn transport_facts_ac_8_the_family_line_and_its_counts() {
    let fixture = shared();
    let lines = fixture.lines();
    let transport = family_lines(&lines, "transport-address");
    assert_eq!(transport.len(), facts(fixture).len());
    for line in lines
        .iter()
        .filter(|l| l.contains("\"transport-address\",\"evidence\""))
    {
        assert!(
            line.starts_with(
                "{\"family\":\"transport-address\",\"evidence\":\"declared\",\"file\":"
            ),
            "{line}"
        );
    }
    let trailer = trailer_of(&lines);
    assert_eq!(
        trailer["counts"]["transport-address"].as_u64(),
        Some(transport.len() as u64)
    );
    let raw_trailer = lines.last().expect("a trailer line");
    assert!(
        raw_trailer.contains(&format!(",\"transport-address\":{}}}", transport.len())),
        "the family is the last counted key: {raw_trailer}"
    );
    let last_test_file = lines
        .iter()
        .rposition(|l| l.starts_with("{\"family\":\"test-file\""))
        .expect("a test-file line");
    let first_transport = lines
        .iter()
        .position(|l| l.starts_with("{\"family\":\"transport-address\""))
        .expect("a transport line");
    assert!(first_transport > last_test_file, "rank after test-file");
}
