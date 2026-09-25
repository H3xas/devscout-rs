//! Integration tests for the `bus-hop` provenance section and `--no-bus`,
//! driven against `fixtures/bus-signals/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    registry: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-bus-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/bus-signals");
        for entry in fs::read_dir(&source).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if Path::new(&name).extension().and_then(|e| e.to_str()) == Some("cs") {
                fs::copy(entry.path(), root.join(&name)).unwrap();
            }
        }
        let registry = root.join("registry.json");
        let fx = Self { root, registry };
        fx.ok(&["init", "--no-hooks"]);
        fx
    }

    fn run(&self, args: &[&str]) -> Output {
        run_in(&self.root, &self.registry, args)
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    // A second root, holding a byte-for-byte copy of this fixture's own
    // mapped `.scout` state, EXCEPT every `bus-hop` edge has been stripped
    // out of `graph.json`. This is the ground truth "prior traversal
    // behaviour" was defined against: a graph that never recorded a
    // bus-hop edge at all, which is exactly what `--no-bus` makes the
    // index behave as if it were reading. No `init`/`map` runs here --
    // `.scout` already carries a finished map, copied wholesale.
    fn bus_free_twin(&self) -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "devscout-bus-free-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        copy_dir_all(&self.root, &root);
        strip_bus_hop_edges(&root.join(".scout/graph/graph.json"));
        let registry = root.join("registry.json");
        Fixture { root, registry }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_file(&self.registry);
    }
}

fn run_in(root: &Path, registry: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_devscout"))
        .current_dir(root)
        .env("SCOUT_REGISTRY", registry)
        .args(args)
        .output()
        .unwrap()
}

fn copy_dir_all(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir_all(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

// Removes every `"kind":"bus-hop"` entry from the persisted edge array --
// the artifact-level equivalent of a repository this feature never touched.
fn strip_bus_hop_edges(graph_path: &Path) {
    let bytes = fs::read(graph_path).unwrap();
    let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let edges = v
        .get_mut("edges")
        .and_then(|e| e.as_array_mut())
        .expect("graph.json must carry an edges array");
    let before = edges.len();
    edges.retain(|e| e.get("kind").and_then(|k| k.as_str()) != Some("bus-hop"));
    assert!(
        edges.len() < before,
        "fixture graph carried no bus-hop edge to strip"
    );
    fs::write(graph_path, serde_json::to_vec(&v).unwrap()).unwrap();
}

#[test]
fn refs_on_a_handler_shows_its_publishers_with_message_and_direction() {
    let fx = Fixture::new();
    let out = fx.ok(&["refs", "ShelfConsumer"]);
    // Two publish sites reach this handler once `PublishingTest.cs` (a
    // later fixture addition) joins `GenericPublish.cs`; this test still
    // checks the ORIGINAL row's own fields, not the row count.
    assert!(out.contains("bus-hop (2):"), "{out}");
    assert!(out.contains("GenericPublish.cs:8"), "{out}");
    assert!(out.contains("  in  "), "direction must be named: {out}");
    assert!(
        out.contains("message=BusSignals.LoanRequested"),
        "the resolved message must be named: {out}"
    );
    assert!(
        out.contains("handler=BusSignals.ShelfConsumer"),
        "the handler must be named: {out}"
    );
    assert!(
        out.contains("evidence=base-arg"),
        "the evidence word must be named, unrespelled: {out}"
    );
}

#[test]
fn impact_on_a_handler_reaches_its_publishers_across_a_bus_hop() {
    let fx = Fixture::new();
    let out = fx.ok(&["impact", "ShelfConsumer", "--json"]);
    assert!(
        out.contains("\"file\":\"GenericPublish.cs\""),
        "the publish site's file must be in the blast radius: {out}"
    );
    assert!(
        out.contains("\"why\":\"bus-hop\""),
        "the row reached through the hop must name it: {out}"
    );
    assert!(
        out.contains("\"busOnly\":true"),
        "the publish site's only path back to the seed is the bus hop itself: {out}"
    );
}

#[test]
fn bus_hop_text_row_states_the_route_is_possible_and_unverified() {
    let fx = Fixture::new();
    let out = fx.ok(&["refs", "ShelfConsumer"]);
    assert!(
        out.contains("possible route, runtime routing unverified"),
        "{out}"
    );
}

#[test]
fn bus_hop_json_row_carries_structured_uncertainty_and_verification_targets() {
    let fx = Fixture::new();
    let out = fx.ok(&["refs", "ShelfConsumer", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let row = v
        .get("bus-hop")
        .and_then(|b| b.get("rows"))
        .and_then(|r| r.as_array())
        .and_then(|rows| rows.first())
        .unwrap_or_else(|| panic!("no bus-hop row: {out}"));
    let possible_route = row
        .get("possibleRoute")
        .unwrap_or_else(|| panic!("no possibleRoute key on the bus-hop row: {out}"));
    assert_eq!(
        possible_route["unverified"],
        serde_json::json!(true),
        "{out}"
    );
    let verify = &possible_route["verify"];
    assert_eq!(
        verify["publisher"],
        serde_json::json!("GenericPublish.cs:8"),
        "{out}"
    );
    assert_eq!(verify["message"], row["message"], "{out}");
    assert_eq!(verify["handler"], row["to"], "{out}");
    assert_eq!(verify["handlerFile"], row["toFile"], "{out}");
    let missing = possible_route["missingEvidence"]
        .as_array()
        .unwrap_or_else(|| panic!("missingEvidence must be an array: {out}"));
    assert!(!missing.is_empty(), "{out}");
}

#[test]
fn bus_hop_compact_row_is_a_marker_with_a_path_to_the_full_row() {
    let fx = Fixture::new();
    let compact = fx.ok(&["refs", "ShelfConsumer", "--compact"]);
    assert!(
        !compact.contains("possible route, runtime routing unverified"),
        "compact must not carry the full disclosure text: {compact}"
    );
    assert!(
        compact.contains("rerun without --compact for the full row"),
        "compact must name a path back to the full row: {compact}"
    );
    let marker_line = compact
        .lines()
        .find(|l| l.trim_start().starts_with("GenericPublish.cs:"))
        .unwrap_or_else(|| panic!("no compact bus-hop line: {compact}"));
    assert!(
        marker_line.contains('?'),
        "the compact row itself carries the short possible-route marker: {marker_line}"
    );
    let full = fx.ok(&["refs", "ShelfConsumer"]);
    assert!(
        full.contains("possible route, runtime routing unverified"),
        "the full row, reproduced by rerunning without --compact, carries the disclosure: {full}"
    );
}

#[test]
fn no_bus_reproduces_the_prior_answer_on_all_four_verbs() {
    let fx = Fixture::new();
    let twin = fx.bus_free_twin();

    let cases: &[&[&str]] = &[
        &["refs", "ShelfConsumer"],
        &["refs", "ShelfConsumer", "--json"],
        &["refs", "ShelfConsumer", "--compact"],
        &["read", "ShelfConsumer"],
        &["read", "ShelfConsumer", "--json"],
        &["impact", "ShelfConsumer"],
        &["impact", "ShelfConsumer", "--json"],
        &["tests", "ShelfConsumer"],
        &["tests", "ShelfConsumer", "--json"],
    ];

    for args in cases {
        let mut suppressed_args: Vec<&str> = (*args).to_vec();
        suppressed_args.push("--no-bus");
        let suppressed = fx.run(&suppressed_args);
        let prior = run_in(&twin.root, &twin.registry, args);

        assert_eq!(
            suppressed.status.code(),
            prior.status.code(),
            "{args:?} --no-bus: exit code must match the pre-feature answer"
        );
        assert_eq!(
            String::from_utf8_lossy(&suppressed.stdout),
            String::from_utf8_lossy(&prior.stdout),
            "{args:?} --no-bus: stdout must be byte-identical to the pre-feature answer"
        );
    }

    // The guard has teeth: the unsuppressed answer actually differs from the
    // bus-free one, so the equality above is not vacuously true of a query
    // the flag never touched.
    let with_bus = fx.ok(&["refs", "ShelfConsumer"]);
    let without_bus_ever = run_in(&twin.root, &twin.registry, &["refs", "ShelfConsumer"]);
    assert_ne!(
        with_bus,
        String::from_utf8_lossy(&without_bus_ever.stdout),
        "the bus-hop section must actually change refs' answer for this symbol"
    );
}

// --- enclosing-scope identity -------------------------------------------

#[test]
fn a_message_nested_in_the_publishing_class_shadows_an_imported_one_of_the_same_name() {
    let fx = Fixture::new();
    let out = fx.ok(&["refs", "Trigger"]);
    assert!(out.contains("bus-hop (1):"), "{out}");
    assert!(
        out.contains("message=BusSignals.ReturnsDesk+LoanRequested"),
        "the publish site's own nested message must win over the top-level one: {out}"
    );
    assert!(
        out.contains("handlers=1"),
        "the nested message must stay distinct from the top-level one's own four handlers: {out}"
    );
}

#[test]
fn a_handler_base_argument_resolves_a_message_nested_beside_the_handler_first() {
    let fx = Fixture::new();
    let out = fx.ok(&["refs", "Handler"]);
    assert!(out.contains("bus-hop (2):"), "{out}");
    assert!(
        out.contains("message=BusSignals.ReturnsDesk+LoanRequested"),
        "the sibling-nested message must win over the top-level one: {out}"
    );
    assert!(
        !out.contains("BusSignals.LoanRequested "),
        "the top-level message must never appear on this handler's own table: {out}"
    );
}

// --- route dedupe, base-list evidence over property-arg -----------------

#[test]
fn a_handler_binding_one_message_on_its_base_and_a_property_emits_one_edge_per_route() {
    let fx = Fixture::new();
    let out = fx.ok(&["refs", "ShelfArchiveConsumer"]);
    assert!(
        out.contains("bus-hop (2):"),
        "one edge per publish site, never two for the base+property pair: {out}"
    );
    assert!(
        !out.contains("property-arg"),
        "base-list evidence must outrank property-arg for the same route: {out}"
    );
    assert!(out.contains("evidence=base-arg"), "{out}");
}

// --- two structural refusals: same-named non-dispatch, test doubles -----

#[test]
fn a_same_named_method_taking_the_message_as_typed_input_emits_no_hop() {
    let fx = Fixture::new();
    let out = fx.ok(&["refs", "AisleLedger"]);
    assert!(
        !out.contains("bus-hop"),
        "a repository-declared method taking the message's own concrete type must earn no hop: {out}"
    );
    assert!(
        out.contains("uses-member (1):"),
        "the call still records as an ordinary reference: {out}"
    );
}

#[test]
fn a_repository_declared_bus_api_generic_over_the_message_still_emits_its_hop() {
    let fx = Fixture::new();
    let out = fx.ok(&["refs", "ShelfConsumer"]);
    assert!(
        out.contains("GenericPublish.cs:8"),
        "the generic bus API's own call must still hop: {out}"
    );
    assert!(out.contains("evidence=base-arg"), "{out}");
}

#[test]
fn a_publish_inside_a_test_double_setup_lambda_emits_no_hop() {
    let fx = Fixture::new();
    for handler in ["ShelfConsumer", "LoanBatchConsumer", "ShelfArchiveConsumer"] {
        let out = fx.ok(&["refs", handler]);
        assert!(
            !out.contains("TestDoubleSetupLambda.cs"),
            "a publish inside a Setup lambda must never reach {handler}: {out}"
        );
    }
}

#[test]
fn a_publish_inside_a_test_double_verify_lambda_emits_no_hop() {
    let fx = Fixture::new();
    for handler in ["ShelfConsumer", "LoanBatchConsumer", "ShelfArchiveConsumer"] {
        let out = fx.ok(&["refs", handler]);
        assert!(
            !out.contains("TestDoubleVerifyLambda.cs"),
            "a publish inside a Verify lambda must never reach {handler}: {out}"
        );
    }
}

#[test]
fn a_publish_inside_a_repository_helper_around_a_test_double_emits_no_hop() {
    let fx = Fixture::new();
    for handler in ["ShelfConsumer", "LoanBatchConsumer", "ShelfArchiveConsumer"] {
        let out = fx.ok(&["refs", handler]);
        assert!(
            !out.contains("TestDoubleHelperWrapper.cs"),
            "a publish inside an Expression<>-wrapped repository helper must never reach {handler}: {out}"
        );
    }
}

// --- publish-site impact reach, forward walk -----------------------------

#[test]
fn impact_on_a_publish_site_reaches_its_candidate_handlers() {
    let fx = Fixture::new();
    let out = fx.ok(&["impact", "GenericPublish.cs", "--json"]);
    assert!(
        out.contains("\"file\":\"ConsumerInterfaceImplementation.cs\""),
        "the publish site must reach its handler's file: {out}"
    );
    assert!(
        out.contains("\"file\":\"BatchConsumer.cs\""),
        "the publish site must reach its batch handler's file too: {out}"
    );
    assert!(
        out.contains("\"busOnly\":true"),
        "a file reached only through the forward hop is a possible route: {out}"
    );
}

#[test]
fn impact_on_a_publishing_symbol_reaches_its_candidate_handlers() {
    let fx = Fixture::new();
    let out = fx.ok(&["impact", "ShelfClerk", "--json"]);
    assert!(
        out.contains("\"file\":\"ConsumerInterfaceImplementation.cs\""),
        "the publishing symbol must reach its handler's file: {out}"
    );
    assert!(
        out.contains("\"file\":\"BatchConsumer.cs\""),
        "the publishing symbol must reach its batch handler's file too: {out}"
    );
}

#[test]
fn no_bus_suppresses_the_forward_publish_site_walk() {
    let fx = Fixture::new();
    // Every reach `GenericPublish.cs` has is over the forward bus-hop walk,
    // so suppressing it leaves nothing at all -- the CLI's own "zero hits"
    // answer (a non-zero exit, not a crash) is the correct outcome here.
    let result = fx.run(&["impact", "GenericPublish.cs", "--no-bus"]);
    let out = String::from_utf8_lossy(&result.stdout);
    assert!(
        !out.contains("ConsumerInterfaceImplementation.cs"),
        "the forward walk must contribute nothing under the suppressor: {out}"
    );
    assert!(
        out.contains("affected files: 0"),
        "no file is reached at all once the only reach is suppressed: {out}"
    );
}

// --- impact/tests possible-route disclosure -------------------------------

#[test]
fn impact_bus_row_text_states_the_route_is_possible_and_unverified() {
    let fx = Fixture::new();
    let out = fx.ok(&["impact", "GenericPublish.cs"]);
    assert!(
        out.contains("possible route, runtime routing unverified"),
        "{out}"
    );
    assert!(
        out.contains("message=BusSignals.LoanRequested"),
        "an impact row carries no message field of its own, so the disclosure must name it: {out}"
    );
    assert!(out.contains("handler=BusSignals."), "{out}");
    assert!(out.contains("handlerFile="), "{out}");
}

#[test]
fn impact_bus_row_json_carries_structured_uncertainty_and_verification_targets() {
    let fx = Fixture::new();
    let out = fx.ok(&["impact", "GenericPublish.cs", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let row = v["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["file"] == "ConsumerInterfaceImplementation.cs")
        .unwrap_or_else(|| panic!("no row for ConsumerInterfaceImplementation.cs: {out}"));
    assert_eq!(row["busOnly"], serde_json::json!(true), "{out}");
    let possible_route = row
        .get("possibleRoute")
        .unwrap_or_else(|| panic!("no possibleRoute key on the bus-only row: {out}"));
    assert_eq!(
        possible_route["unverified"],
        serde_json::json!(true),
        "{out}"
    );
    let verify = &possible_route["verify"];
    assert_eq!(
        verify["publisher"],
        serde_json::json!("GenericPublish.cs:8"),
        "{out}"
    );
    assert_eq!(
        verify["handler"],
        serde_json::json!("BusSignals.ShelfConsumer"),
        "{out}"
    );
    assert_eq!(
        verify["handlerFile"],
        serde_json::json!("ConsumerInterfaceImplementation.cs"),
        "{out}"
    );
    let missing = possible_route["missingEvidence"].as_array().unwrap();
    assert!(!missing.is_empty(), "{out}");
}

#[test]
fn impact_bus_row_compact_is_a_marker_with_a_path_to_the_full_row() {
    let fx = Fixture::new();
    let compact = fx.ok(&["impact", "GenericPublish.cs", "--compact"]);
    assert!(
        !compact.contains("possible route, runtime routing unverified"),
        "compact must not carry the full disclosure text: {compact}"
    );
    assert!(
        compact.contains("rerun without --compact for the full row"),
        "compact must name a path back to the full row: {compact}"
    );
    let marker_line = compact
        .lines()
        .find(|l| {
            l.trim_start()
                .starts_with("ConsumerInterfaceImplementation.cs")
        })
        .unwrap_or_else(|| panic!("no compact bus-only line: {compact}"));
    assert!(
        marker_line.contains('?'),
        "the compact row itself carries the short possible-route marker: {marker_line}"
    );
    let full = fx.ok(&["impact", "GenericPublish.cs"]);
    assert!(
        full.contains("possible route, runtime routing unverified"),
        "the full row, reproduced by rerunning without --compact, carries the disclosure: {full}"
    );
}

#[test]
fn tests_bus_row_text_states_the_route_is_possible_and_unverified() {
    let fx = Fixture::new();
    let out = fx.ok(&["tests", "ShelfConsumer"]);
    assert!(
        out.contains("possible route, runtime routing unverified"),
        "{out}"
    );
    assert!(out.contains("PublishingTest.cs:15"), "{out}");
}

#[test]
fn tests_bus_row_json_carries_structured_uncertainty_and_verification_targets() {
    let fx = Fixture::new();
    let out = fx.ok(&["tests", "ShelfConsumer", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let row = v["bus-hop"]["rows"]
        .as_array()
        .and_then(|rows| rows.first())
        .unwrap_or_else(|| panic!("no bus-hop row: {out}"));
    let possible_route = row
        .get("possibleRoute")
        .unwrap_or_else(|| panic!("no possibleRoute key on the tests bus-hop row: {out}"));
    assert_eq!(
        possible_route["unverified"],
        serde_json::json!(true),
        "{out}"
    );
    let verify = &possible_route["verify"];
    assert_eq!(
        verify["publisher"],
        serde_json::json!("PublishingTest.cs:15"),
        "{out}"
    );
    assert_eq!(verify["message"], row["message"], "{out}");
    assert_eq!(verify["handler"], row["to"], "{out}");
    assert_eq!(verify["handlerFile"], row["toFile"], "{out}");
}

#[test]
fn tests_bus_row_compact_is_a_marker_with_a_path_to_the_full_row() {
    let fx = Fixture::new();
    let compact = fx.ok(&["tests", "ShelfConsumer", "--compact"]);
    assert!(
        !compact.contains("possible route, runtime routing unverified"),
        "compact must not carry the full disclosure text: {compact}"
    );
    assert!(
        compact.contains("rerun without --compact for the full row"),
        "compact must name a path back to the full row: {compact}"
    );
    let marker_line = compact
        .lines()
        .find(|l| l.trim_start().starts_with("PublishingTest.cs:"))
        .unwrap_or_else(|| panic!("no compact tests bus-hop line: {compact}"));
    assert!(
        marker_line.contains('?'),
        "the compact row itself carries the short possible-route marker: {marker_line}"
    );
}

// --- `tests` traverses bus hops -------------------------------------------

#[test]
fn tests_lists_a_test_file_publishing_to_the_seed_handler_as_a_possible_route() {
    let fx = Fixture::new();
    let out = fx.ok(&["tests", "ShelfConsumer"]);
    assert!(out.contains("bus-hop (1):"), "{out}");
    assert!(out.contains("PublishingTest.cs:15"), "{out}");
}

#[test]
fn tests_counts_exclude_possible_route_rows() {
    let fx = Fixture::new();
    let out = fx.ok(&["tests", "ShelfConsumer", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["testFileCount"], serde_json::json!(0), "{out}");
    assert_eq!(v["refCount"], serde_json::json!(0), "{out}");
    assert_eq!(v["heuristicFileCount"], serde_json::json!(0), "{out}");
    assert_eq!(v["heuristicRefCount"], serde_json::json!(0), "{out}");
    assert_eq!(v["bus-hop"]["total"], serde_json::json!(1), "{out}");
}
