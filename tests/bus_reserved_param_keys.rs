//! Integration tests for the parameter route of the consumer-base admission
//! rule against the reserved `methodParams` entries, driven against
//! `fixtures/bus-reserved-param-keys/`.
//!
//! A method result, property or field type is not a declared parameter, so a
//! sent message named only there must not admit a registered type's base. A
//! method whose name is a verbatim identifier still declares parameters.

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
            "devscout-bus-reserved-param-keys-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/bus-reserved-param-keys");
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

    fn ok(&self, args: &[&str]) {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // Every bus hop as `(handler simple name, message simple name)`, sorted.
    fn hops(&self) -> Vec<(String, String)> {
        let bytes = fs::read(self.root.join(".scout/graph/graph.json")).unwrap();
        let graph: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let mut out: Vec<(String, String)> = graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "bus-hop")
            .map(|e| {
                (
                    simple(e["to"].as_str().unwrap()),
                    simple(e["message"].as_str().unwrap()),
                )
            })
            .collect();
        out.sort();
        out
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn simple(id: &str) -> String {
    id.rsplit_once('.').map_or(id, |(_, t)| t).to_string()
}

fn hop(to: &str, message: &str) -> (String, String) {
    (to.to_string(), message.to_string())
}

#[test]
fn reserved_param_keys_ac_1_a_message_named_only_in_a_method_result_admits_no_base() {
    let hops = Fixture::new().hops();
    assert!(
        !hops.contains(&hop("ShoalIndexer", "ShoalNotice")),
        "ShoalIndexer names ShoalNotice only in a result type: {hops:?}"
    );
    assert!(
        !hops.contains(&hop("ReefShoalWatcher", "ReefNotice")),
        "ShoalBase has no admitting registration, so its unregistered carrier gets no hop: {hops:?}"
    );
}

#[test]
fn reserved_param_keys_ac_3_declared_parameters_still_admit() {
    let hops = Fixture::new().hops();
    assert_eq!(
        hops,
        vec![
            hop("CurrentLogger", "CurrentNotice"),
            hop("ReefGuard", "ReefNotice"),
        ],
        "a verbatim-named method and a real parameter beside a result-only member both admit"
    );
}
