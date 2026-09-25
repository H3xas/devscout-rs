//! Integration tests for the admission rule that gates a registered type's
//! own generic base from the consumer-base vocabulary, driven against
//! `fixtures/bus-vocabulary-admission/`.
//!
//! A base is not admitted on argument shape alone: the registered type must
//! not itself be a message the corpus sends, and it must actually receive,
//! through a parameter or a bound property, a message the corpus does send.
//! `fixtures/bus-vocabulary/` and `tests/bus_vocabulary.rs` are untouched --
//! every case here lives in its own directory and file.

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
            "devscout-bus-vocab-admission-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/bus-vocabulary-admission");
        for entry in fs::read_dir(&source).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let name = name.to_str().unwrap();
            if name.ends_with(".cs") {
                fs::copy(entry.path(), root.join(name)).unwrap();
            }
        }
        let registry = root.join("registry.json");
        let fx = Self { root, registry };
        fx.ok(&["init", "--no-hooks"]);
        fx.ok(&["map", "."]);
        fx
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .current_dir(&self.root)
            .env("SCOUT_REGISTRY", &self.registry)
            .args(args)
            .output()
            .unwrap()
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

    fn graph(&self) -> serde_json::Value {
        let bytes = fs::read(self.root.join(".scout/graph/graph.json")).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // Every bus hop as `(publishing file, handler simple name, message
    // simple name, evidence)`, sorted so a case can assert on the whole set.
    fn hops(&self) -> Vec<(String, String, String, String)> {
        let graph = self.graph();
        let mut out: Vec<(String, String, String, String)> = graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "bus-hop")
            .map(|e| {
                (
                    e["from_file"].as_str().unwrap().to_string(),
                    simple(e["to"].as_str().unwrap()),
                    simple(e["message"].as_str().unwrap()),
                    e["evidence"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        out.sort();
        out
    }

    fn handlers_of(&self, message: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .hops()
            .into_iter()
            .filter(|(_, _, m, _)| m == message)
            .map(|(_, to, _, _)| to)
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_file(&self.registry);
    }
}

fn simple(id: &str) -> String {
    id.rsplit_once('.').map_or(id, |(_, t)| t).to_string()
}

#[test]
fn a_sent_message_equating_itself_is_never_its_own_handler() {
    let fx = Fixture::new();
    let handlers = fx.handlers_of("PilotRequest");
    assert!(
        !handlers.contains(&"PilotRequest".to_string()),
        "a sent, self-equatable message must never route to itself: {handlers:?}"
    );
    assert_eq!(
        handlers,
        vec!["PilotOfficer".to_string()],
        "the real handler still reaches it: {handlers:?}"
    );
}

#[test]
fn a_base_read_off_a_registered_message_stays_outside_the_vocabulary() {
    let fx = Fixture::new();
    let handlers = fx.handlers_of("BerthNotice");
    assert!(
        !handlers.contains(&"LedgerReader".to_string()),
        "LedgerBase's only registration named a sent message, so it never enters the vocabulary, \
         even for an unrelated, otherwise-valid carrier: {handlers:?}"
    );
}

#[test]
fn a_registered_base_whose_argument_no_publish_site_sends_stays_outside_the_vocabulary() {
    let fx = Fixture::new();
    assert!(
        fx.handlers_of("TerminalNotice").is_empty(),
        "TerminalNotice is never sent, so TerminalWatcher earns no hop"
    );
    let handlers = fx.handlers_of("BerthNotice");
    assert!(
        !handlers.contains(&"SecondTerminalWatcher".to_string()),
        "TerminalBase's only registration's argument was never sent, so the base never enters \
         the vocabulary, even for an unrelated, otherwise-valid carrier: {handlers:?}"
    );
}

#[test]
fn a_registered_base_whose_carrier_never_receives_its_argument_stays_outside_the_vocabulary() {
    let fx = Fixture::new();
    let tide_handlers = fx.handlers_of("TideNotice");
    assert!(
        !tide_handlers.contains(&"SilentWatcher".to_string()),
        "SilentWatcher never receives its own argument as a parameter, only returns it: {tide_handlers:?}"
    );
    let berth_handlers = fx.handlers_of("BerthNotice");
    assert!(
        !berth_handlers.contains(&"SecondSilentWatcher".to_string()),
        "SilentBase's only registration never received its argument, so the base never enters \
         the vocabulary, even for an unrelated, otherwise-valid carrier: {berth_handlers:?}"
    );
}

#[test]
fn a_binding_only_bases_own_type_argument_earns_no_route() {
    let fx = Fixture::new();
    let tide_handlers = fx.handlers_of("TideNotice");
    assert!(
        !tide_handlers.contains(&"EchoWatcher".to_string()),
        "ChimeWatcherBase is binding-only: EchoWatcher's own base argument (TideNotice, sent) \
         must earn no route: {tide_handlers:?}"
    );
}

#[test]
fn a_registered_flow_binding_a_sent_message_on_a_property_reaches_its_publisher() {
    let fx = Fixture::new();
    let by_property: Vec<(String, String)> = fx
        .hops()
        .into_iter()
        .filter(|(_, _, _, evidence)| evidence == "property-arg")
        .map(|(_, to, message, _)| (to, message))
        .collect();
    assert_eq!(
        by_property,
        vec![("EchoWatcher".to_string(), "EchoNotice".to_string())],
        "EchoWatcher's bound property is what admits ChimeWatcherBase, and reaches its publisher: {by_property:?}"
    );
}

#[test]
fn a_carrier_receiving_its_message_inside_a_wrapper_parameter_admits_its_base() {
    let fx = Fixture::new();
    let handlers = fx.handlers_of("BerthNotice");
    assert!(
        handlers.contains(&"BerthBatchHandler".to_string()),
        "BerthBatchHandler receives BerthNotice wrapped in List<T>, which the receiving check \
         must see through: {handlers:?}"
    );
}
