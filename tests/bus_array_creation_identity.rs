//! Integration tests for message identity's array rank on two array-creation
//! spellings: an explicit array creation with an initializer, and an
//! implicitly typed array creation, each never reaching a single-message
//! consumer, driven against `fixtures/bus-array-creation-identity/`.
//!
//! `fixtures/bus-signals/`, `fixtures/bus-vocabulary/`,
//! `fixtures/bus-vocabulary-admission/`, `fixtures/bus-array-identity/`,
//! `fixtures/bus-extension-refusal/` and
//! `fixtures/bus-optional-parameter-alignment/` are untouched -- every case
//! here lives in its own directory and file, with its own element types, so
//! it cannot collide with `bus-array-identity`'s own suffix-matching tests.

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
            "devscout-bus-array-creation-identity-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/bus-array-creation-identity");
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

    // Every bus hop as `(handler simple name, message id)`, sorted. The
    // message id is left FULL (not shortened to a simple name) so a case can
    // assert on its own array suffix.
    fn hops(&self) -> Vec<(String, String)> {
        let graph = self.graph();
        let mut out: Vec<(String, String)> = graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "bus-hop")
            .map(|e| {
                (
                    simple(e["to"].as_str().unwrap()),
                    e["message"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        out.sort();
        out
    }

    // Matched by exact trailing text, never a bare `contains`: `TideGauge`
    // and `TideGauge[]` must stay two different suffixes so a case can tell
    // which identity a hop actually carries.
    fn handlers_of(&self, message_id_suffix: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .hops()
            .into_iter()
            .filter(|(_, m)| m.ends_with(message_id_suffix))
            .map(|(to, _)| to)
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
fn an_explicit_array_creation_with_an_initializer_never_reaches_a_single_message_consumer() {
    let fx = Fixture::new();
    let array_handlers = fx.handlers_of("BeaconSighting[]");
    assert_eq!(
        array_handlers,
        vec!["BeaconBulletin".to_string()],
        "the explicit array creation with an initializer must reach the array consumer, \
         suffix included: {array_handlers:?}"
    );
    let single_handlers = fx.handlers_of("BeaconSighting");
    assert_eq!(
        single_handlers,
        vec!["BeaconOfficer".to_string()],
        "the array creation must never reach the single-message consumer, and the plain \
         single publish in the same fixture must still reach its own consumer: \
         {single_handlers:?}"
    );
}

#[test]
fn an_implicitly_typed_array_creation_never_reaches_a_single_message_consumer() {
    let fx = Fixture::new();
    let array_handlers = fx.handlers_of("TideGauge[]");
    assert_eq!(
        array_handlers,
        vec!["TideGaugeBulletin".to_string()],
        "the implicitly typed array creation must reach the array consumer, suffix \
         included: {array_handlers:?}"
    );
    let single_handlers = fx.handlers_of("TideGauge");
    assert_eq!(
        single_handlers,
        vec!["TideOfficer".to_string()],
        "the array creation must never reach the single-message consumer, and the plain \
         single publish in the same fixture must still reach its own consumer: \
         {single_handlers:?}"
    );
}
