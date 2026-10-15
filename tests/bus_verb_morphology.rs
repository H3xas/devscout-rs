//! Publish verb word matching through extraction and resolution.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    registry: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-bus-morphology-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/bus-verb-morphology");
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            if entry.path().extension().is_some_and(|ext| ext == "cs") {
                fs::copy(entry.path(), root.join(entry.file_name())).unwrap();
            }
        }
        let registry = root.join("registry.json");
        let fixture = Self { root, registry };
        fixture.run(&["init", "--no-hooks"]);
        fixture.run(&["map", "."]);
        fixture
    }

    fn run(&self, args: &[&str]) {
        let output = Command::new(env!("CARGO_BIN_EXE_devscout"))
            .current_dir(&self.root)
            .env("SCOUT_REGISTRY", &self.registry)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn hops(&self) -> Vec<(String, String, String)> {
        let graph: serde_json::Value =
            serde_json::from_slice(&fs::read(self.root.join(".scout/graph/graph.json")).unwrap())
                .unwrap();
        graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|edge| edge["kind"] == "bus-hop")
            .map(|edge| {
                (
                    edge["from_file"].as_str().unwrap().to_string(),
                    edge["message"].as_str().unwrap().to_string(),
                    edge["to"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn from_file<'a>(
    hops: &'a [(String, String, String)],
    file: &str,
) -> Vec<&'a (String, String, String)> {
    hops.iter().filter(|hop| hop.0 == file).collect()
}

#[test]
fn rooted_house_verb_reaches_its_handler() {
    let fixture = Fixture::new();
    let hops = fixture.hops();
    for file in ["TryRoot.cs", "MoreRoot.cs"] {
        let matching = from_file(&hops, file);
        assert_eq!(matching.len(), 3, "{file}: {matching:?}");
        for (_, message, handler) in matching {
            assert!(message.ends_with(".ParcelNotice"), "{file}: {message}");
            assert!(handler.ends_with(".ParcelWorker"), "{file}: {handler}");
        }
    }
}

#[test]
fn guarded_verb_emits_no_hop() {
    let fixture = Fixture::new();
    assert!(from_file(&fixture.hops(), "Guarded.cs").is_empty());
}

#[test]
fn root_inside_a_longer_word_emits_no_hop() {
    let fixture = Fixture::new();
    assert!(from_file(&fixture.hops(), "EmbeddedRoot.cs").is_empty());
}

#[test]
fn rootless_generic_call_emits_no_hop() {
    let fixture = Fixture::new();
    assert!(from_file(&fixture.hops(), "Rootless.cs").is_empty());
}

#[test]
fn wrapper_around_a_rooted_house_verb_is_not_promoted() {
    let fixture = Fixture::new();
    assert!(from_file(&fixture.hops(), "Wrapper.cs").is_empty());
}
