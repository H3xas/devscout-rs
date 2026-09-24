//! Integration tests for message-identity's array rank: a single message and
//! an array of that same message are different identities on both sides of
//! a `bus-hop`, driven against `fixtures/bus-array-identity/`.
//!
//! `fixtures/bus-signals/`, `fixtures/bus-vocabulary/` and
//! `fixtures/bus-vocabulary-admission/` are untouched -- every case here
//! lives in its own directory and file.

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
            "devscout-bus-array-identity-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/bus-array-identity");
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

    // Every bus hop as `(publishing file:line, handler simple name, message
    // id, evidence)`, sorted so a case can assert on the whole set. The
    // message id is left FULL (not shortened to a simple name) so a case
    // can assert on its own array suffix.
    fn hops(&self) -> Vec<(String, String, String, String)> {
        let graph = self.graph();
        let mut out: Vec<(String, String, String, String)> = graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "bus-hop")
            .map(|e| {
                (
                    format!(
                        "{}:{}",
                        e["from_file"].as_str().unwrap(),
                        e["from_line"].as_u64().unwrap()
                    ),
                    simple(e["to"].as_str().unwrap()),
                    e["message"].as_str().unwrap().to_string(),
                    e["evidence"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        out.sort();
        out
    }

    // Matched by exact trailing text, never a bare `contains`: `AlertNotice`
    // and `AlertNotice[]` must stay two different suffixes so a case can
    // tell which identity a hop actually carries.
    fn handlers_of(&self, message_id_suffix: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .hops()
            .into_iter()
            .filter(|(_, _, m, _)| m.ends_with(message_id_suffix))
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
fn a_single_message_publish_never_reaches_an_array_consumer() {
    let fx = Fixture::new();
    let single_handlers = fx.handlers_of("AlertNotice");
    assert_eq!(
        single_handlers,
        vec!["AlertOfficer".to_string()],
        "the plain single-message publish must still reach its ordinary consumer: {single_handlers:?}"
    );
    let array_handlers = fx.handlers_of("AlertNotice[]");
    assert!(
        array_handlers.is_empty(),
        "a single-message publish must never reach an array consumer: {array_handlers:?}"
    );
}

#[test]
fn an_array_publish_never_reaches_a_single_message_consumer() {
    let fx = Fixture::new();
    let array_handlers = fx.handlers_of("StormWarning[]");
    assert_eq!(
        array_handlers,
        vec!["StormBulletin".to_string()],
        "the array publish must reach the array consumer: {array_handlers:?}"
    );
    let single_handlers = fx.handlers_of("StormWarning");
    assert!(
        single_handlers.is_empty(),
        "the array publish must never reach the single-message consumer, even though this \
         spelling (a declared array-typed local) used to lose its own array bit at extraction: \
         {single_handlers:?}"
    );
}

#[test]
fn an_array_publish_reaches_the_array_consumer_of_its_element_type() {
    let fx = Fixture::new();
    let hops = fx.hops();
    let tide_hops: Vec<&(String, String, String, String)> = hops
        .iter()
        .filter(|(_, to, _, _)| to == "TideBulletin")
        .collect();
    assert_eq!(
        tide_hops.len(),
        4,
        "all four array-publish spellings (generic argument, explicit creation, implicit \
         creation, declared variable) must each reach the array consumer once: {tide_hops:?}"
    );
    for (_, _, message, _) in &tide_hops {
        assert!(
            message.ends_with("TideNotice[]"),
            "every edge's own message must carry the array suffix: {message}"
        );
    }
}

#[test]
fn an_implicit_array_of_mixed_constructions_names_no_message() {
    let fx = Fixture::new();
    let hops = fx.hops();
    let from_mixed: Vec<&(String, String, String, String)> = hops
        .iter()
        .filter(|(from, _, _, _)| from.contains("MixedImplicitArray.cs"))
        .collect();
    assert!(
        from_mixed.is_empty(),
        "an implicit array creation whose elements construct different types names no \
         message and earns no hop at all: {from_mixed:?}"
    );
}

#[test]
fn a_property_bound_array_message_is_bound_apart_from_its_element() {
    let fx = Fixture::new();
    let hops = fx.hops();
    let harbor_hops: Vec<&(String, String, String, String)> = hops
        .iter()
        .filter(|(_, to, _, _)| to == "HarborWarden")
        .collect();
    let single: Vec<&&(String, String, String, String)> = harbor_hops
        .iter()
        .filter(|(_, _, message, _)| message.ends_with(".HarborNotice"))
        .collect();
    assert_eq!(
        single.len(),
        1,
        "single publish reaches HarborWarden once: {single:?}"
    );
    assert_eq!(
        single[0].3, "base-arg",
        "the single message reaches it through the base argument: {single:?}"
    );
    let array: Vec<&&(String, String, String, String)> = harbor_hops
        .iter()
        .filter(|(_, _, message, _)| message.ends_with("HarborNotice[]"))
        .collect();
    assert_eq!(
        array.len(),
        1,
        "array publish reaches HarborWarden once: {array:?}"
    );
    assert_eq!(
        array[0].3, "property-arg",
        "the array reaches it only through the bound property, never through the base \
         argument the single message uses: {array:?}"
    );
}

#[test]
fn a_type_parameter_array_names_no_message_and_makes_no_wrapper() {
    let fx = Fixture::new();
    let hops = fx.hops();
    let envelope_hops: Vec<&(String, String, String, String)> = hops
        .iter()
        .filter(|(_, to, _, _)| to == "Envelope")
        .collect();
    assert!(
        envelope_hops.is_empty(),
        "an open generic's own type-parameter array names no message and makes no wrapper: \
         {envelope_hops:?}"
    );
    let relief_handlers = fx.handlers_of("ReliefRequest");
    assert_eq!(
        relief_handlers,
        vec!["ReliefOfficer".to_string()],
        "the rest of the fixture still routes normally: {relief_handlers:?}"
    );
}
