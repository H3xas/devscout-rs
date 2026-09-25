//! Shape test for the committed flow-tracer fact snapshot
//! `fixtures/csharp-flowtrace/facts.json`.
//!
//! The snapshot is produced by the Roslyn sidecar (`tools/scout-semantic`)
//! running in its `--emit flowtrace-facts` mode over the fixture solution in
//! the same directory; CI's `semantic-audit` job regenerates it and diffs the
//! bytes. This file never invokes `dotnet`: it reads the committed document
//! and pins the contract the flow tracer relies on -- header keys, the three
//! fields every fact carries, the required fields of every emitted kind, the
//! sort order that makes the file reproducible, and that each kind the mode
//! promises is actually present -- so a sidecar change that drifts the shape
//! fails `cargo test` even on a machine without a .NET toolchain.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

fn snapshot_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-flowtrace/facts.json")
}

fn text() -> String {
    let text = fs::read_to_string(snapshot_path()).expect("read facts snapshot");
    assert!(
        text.ends_with('\n') && !text.contains('\r'),
        "snapshot is LF-terminated"
    );
    text
}

/// The key a line of the two-space-indented document declares at `depth`
/// levels, if it declares one. `serde_json` sorts object keys, so key order
/// -- part of the contract, since the header reads first and every fact
/// leads with `type`, `file`, `line` -- is read off the text itself.
fn key_at(line: &str, depth: usize) -> Option<&str> {
    let indent = " ".repeat(depth * 2);
    let rest = line.strip_prefix(indent.as_str())?;
    if rest.starts_with(' ') {
        return None;
    }
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    rest[end + 1..].starts_with(": ").then_some(&rest[..end])
}

fn header_keys(text: &str) -> Vec<&str> {
    text.lines().filter_map(|line| key_at(line, 1)).collect()
}

/// Key order of every fact object, in document order.
fn fact_keys(text: &str) -> Vec<Vec<&str>> {
    let mut facts = Vec::new();
    for line in text.lines() {
        if line == "    {" {
            facts.push(Vec::new());
        } else if let Some(key) = key_at(line, 3) {
            facts.last_mut().expect("a fact object is open").push(key);
        }
    }
    facts
}

fn load() -> Map<String, Value> {
    match serde_json::from_str(&text()).expect("snapshot parses as JSON") {
        Value::Object(map) => map,
        other => panic!("snapshot root is not an object: {other}"),
    }
}

fn facts(doc: &Map<String, Value>) -> Vec<&Map<String, Value>> {
    doc["facts"]
        .as_array()
        .expect("facts is an array")
        .iter()
        .map(|fact| fact.as_object().expect("every fact is an object"))
        .collect()
}

/// Required fields per fact kind, mirroring the flow tracer's schema table
/// for the kinds this mode emits; `type`, `file` and `line` are required on
/// every fact and checked separately.
fn required_fields() -> BTreeMap<&'static str, &'static [&'static str]> {
    BTreeMap::from([
        ("consume", &["message", "consumer"][..]),
        ("ctor_field", &["class", "field", "paramType"][..]),
        ("di_binding", &["iface", "impl"][..]),
        ("iface_impl", &["class", "iface"][..]),
        ("message_class", &["name", "fqn"][..]),
        (
            "method_call",
            &["class", "method", "field", "calledMethod"][..],
        ),
        ("method_span", &["class", "method", "endLine"][..]),
        ("publish", &["message"][..]),
        ("route", &["controller", "action", "verb", "template"][..]),
    ])
}

#[test]
fn header_keys_come_first_and_name_the_producer() {
    let text = text();
    let doc = load();
    assert_eq!(
        header_keys(&text),
        [
            "schemaVersion",
            "producer",
            "version",
            "repo",
            "kind",
            "generatedFrom",
            "compilation",
            "facts"
        ],
        "header key order (git identity is absent: the snapshot is generated with --no-git)"
    );
    assert_eq!(doc["schemaVersion"], Value::from(1));
    assert_eq!(doc["producer"], Value::from("scout-semantic"));
    assert_eq!(doc["kind"], Value::from("backend"));
    assert_eq!(doc["repo"], Value::from("csharp-flowtrace"));
    let version = doc["version"].as_str().expect("version is a string");
    assert!(
        !version.contains('+'),
        "version carries no source-revision suffix: {version}"
    );
    assert_eq!(
        doc["generatedFrom"],
        Value::from(format!("scout-semantic {version}"))
    );
    let compilation = doc["compilation"]
        .as_object()
        .expect("compilation is an object");
    assert_eq!(compilation["solution"], Value::from("Fixture.sln"));
    let units: Vec<&str> = compilation["units"]
        .as_array()
        .expect("units is an array")
        .iter()
        .map(|unit| unit.as_str().expect("unit is a string"))
        .collect();
    assert_eq!(units, ["Api|net9.0", "Shared|net9.0"]);
    let digest = compilation["digest"].as_str().expect("digest is a string");
    assert_eq!(digest.len(), 40, "digest is a hex sha1: {digest}");
    assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
    for absent in [
        "headSha",
        "dirty",
        "dirtyDigest",
        "fileCount",
        "generatedAt",
    ] {
        assert!(
            !doc.contains_key(absent),
            "{absent} is absent under --no-git"
        );
    }
}

#[test]
fn every_fact_carries_type_file_line_and_its_required_fields() {
    let doc = load();
    let required = required_fields();
    for (index, fact) in facts(&doc).iter().enumerate() {
        let kind = fact["type"]
            .as_str()
            .unwrap_or_else(|| panic!("fact #{index}: type"));
        let fields = required
            .get(kind)
            .unwrap_or_else(|| panic!("fact #{index}: unknown fact type {kind}"));
        let file = fact["file"]
            .as_str()
            .unwrap_or_else(|| panic!("fact #{index}: file"));
        assert!(
            !file.is_empty() && !file.contains('\\') && !file.starts_with('/'),
            "fact #{index}: file is repository-relative with forward slashes: {file}"
        );
        let line = fact["line"]
            .as_u64()
            .unwrap_or_else(|| panic!("fact #{index}: line"));
        assert!(line >= 1, "fact #{index}: line is 1-based");
        for field in *fields {
            let value = fact
                .get(*field)
                .unwrap_or_else(|| panic!("fact #{index} ({kind}): missing {field}"));
            assert!(!value.is_null(), "fact #{index} ({kind}): {field} is null");
        }
        if let Some(end) = fact.get("endLine") {
            assert!(end.as_u64().expect("endLine is an integer") >= line);
        }
        assert!(
            !fact.contains_key("provenance"),
            "fact #{index}: no fact self-declares provenance"
        );
    }
}

#[test]
fn every_fact_leads_with_type_file_line_then_its_required_fields() {
    let text = text();
    let doc = load();
    let all = facts(&doc);
    let required = required_fields();
    let orders = fact_keys(&text);
    assert_eq!(orders.len(), all.len(), "one key list per fact");
    for (index, keys) in orders.iter().enumerate() {
        assert_eq!(
            &keys[..3],
            ["type", "file", "line"],
            "fact #{index}: key order"
        );
        let kind = all[index]["type"].as_str().unwrap();
        let fields = required[kind];
        assert_eq!(
            &keys[3..3 + fields.len()],
            fields,
            "fact #{index} ({kind}): required fields follow in schema order"
        );
    }
}

#[test]
fn facts_are_sorted_and_distinct() {
    let doc = load();
    let all = facts(&doc);
    let key = |fact: &Map<String, Value>| {
        (
            fact["file"].as_str().unwrap().to_owned(),
            fact["line"].as_u64().unwrap(),
            fact["type"].as_str().unwrap().to_owned(),
            serde_json::to_string(fact).unwrap(),
        )
    };
    for pair in all.windows(2) {
        let (a, b) = (key(pair[0]), key(pair[1]));
        assert!(
            a < b,
            "facts are strictly ascending by (file, line, type, json):\n{a:?}\n{b:?}"
        );
    }
}

#[test]
fn every_promised_kind_is_present() {
    let doc = load();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for fact in facts(&doc) {
        *counts.entry(fact["type"].as_str().unwrap()).or_default() += 1;
    }
    for kind in required_fields().keys() {
        assert!(
            counts.get(kind).copied().unwrap_or(0) >= 1,
            "kind {kind} has at least one fact; counts: {counts:?}"
        );
    }
}

#[test]
fn semantic_resolution_shows_where_regexes_stop() {
    let doc = load();
    let all = facts(&doc);
    let find = |kind: &str, pred: &dyn Fn(&Map<String, Value>) -> bool| {
        all.iter()
            .find(|fact| fact["type"] == kind && pred(fact))
            .copied()
    };
    // A primary-constructor consumer is a consume fact like any other.
    let primary = find("consume", &|f| f["consumer"] == "ParcelDispatchedConsumer")
        .expect("primary-constructor consumer");
    assert_eq!(primary["message"], "ParcelDispatchedMessage");
    // A message published through a local variable resolves to the local's type.
    assert!(
        find("publish", &|f| f["message"] == "RouteNote").is_some(),
        "publish of a local variable"
    );
    // A publish through an extra publish-call name resolves the argument's type.
    assert!(
        find("publish", &|f| f["message"] == "ParcelLostEvent").is_some(),
        "publish through an extra publish call"
    );
    // A minimal-API lambda parameter of a fixture type becomes an injected field
    // under the file's own controller name, while framework types do not.
    let lambda_field = find("ctor_field", &|f| {
        f["class"] == "Program" && f["field"] == "repository"
    })
    .expect("lambda parameter as ctor_field");
    assert_eq!(lambda_field["paramType"], "IParcelRepository");
    assert!(
        find("ctor_field", &|f| f["field"] == "http"
            || f["field"] == "ct")
        .is_none(),
        "framework-typed lambda parameters are not injected fields"
    );
    // A route group chain composes into the template.
    assert!(
        find("route", &|f| f["template"] == "api/labels/{id}"
            && f["verb"] == "DELETE")
        .is_some(),
        "inline MapGroup chain prefix"
    );
    assert!(
        find("route", &|f| f["template"] == "api/parcels/{id}"
            && f["verb"] == "GET")
        .is_some(),
        "MapGroup prefix through a local"
    );
    // Attribute routes expand [controller] and [action].
    assert!(
        find("route", &|f| f["template"] == "api/Parcels/Archive"
            && f["verb"] == "ANY")
        .is_some(),
        "[action] expansion with no verb attribute"
    );
    // A group declared in a referenced project is read as syntax, so its
    // literal prefix survives the compilation boundary.
    assert!(
        find("route", &|f| f["template"] == "admin/stats").is_some(),
        "MapGroup prefix declared in another project"
    );
    // Two group properties that reference each other terminate at the first
    // revisit instead of unrolling to the depth cap.
    assert!(
        find("route", &|f| f["template"] == "right/left/cycle").is_some(),
        "cyclic MapGroup chain"
    );
    // A namespace that merely starts with the letters of a framework root is
    // not a framework namespace.
    let invoice = find("ctor_field", &|f| f["field"] == "invoice")
        .expect("lambda parameter from a namespace starting with `System`");
    assert_eq!(invoice["paramTypeFqn"], "Systematic.Billing.Invoice");
    // A call to a member on a constructor-injected field is a method_call,
    // naming the calling method and the called member -- the declared,
    // previously unpopulated slot this producer now fills.
    let method_call = find("method_call", &|f| {
        f["class"] == "DeliveryScheduledConsumer" && f["field"] == "_repository"
    })
    .expect("a call on a ctor-injected field");
    assert_eq!(method_call["method"], "NotifyLost");
    assert_eq!(method_call["calledMethod"], "Find");
    // A field injected without a null-guard (plain `_bus = bus;`) still
    // counts as constructor-injected, so a call on it is method_call too.
    assert!(
        find("method_call", &|f| f["field"] == "_bus"
            && f["calledMethod"] == "Publish")
        .is_some(),
        "a directly assigned (no null-guard) ctor-injected field still yields method_call"
    );
    // An expression-bodied method (`=>`, no `{ }` block) calling a
    // ctor-injected field still yields method_call -- the same slot a
    // block-bodied method fills, read from the arrow body instead.
    let expression_bodied = find("method_call", &|f| {
        f["class"] == "ParcelsController" && f["method"] == "HasRecord"
    })
    .expect("an expression-bodied method calling a ctor-injected field");
    assert_eq!(expression_bodied["field"], "_repository");
    assert_eq!(expression_bodied["calledMethod"], "Find");
    // A partial consumer yields one consume fact, on the part carrying the
    // base list.
    let partial: Vec<_> = all
        .iter()
        .filter(|f| f["type"] == "consume" && f["consumer"] == "ReturnRequestedConsumer")
        .collect();
    assert_eq!(partial.len(), 1, "one consume fact per partial consumer");
    assert_eq!(
        partial[0]["file"],
        "src/Api/Consumers/ReturnRequestedConsumer.cs"
    );
}
