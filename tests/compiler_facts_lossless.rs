//! Compiler diagnostics, complete symbol identity, occurrence spans, the
//! embedded compilation-context envelope, and explicit uncertainty states
//! survive production, publication, admission and read-back without
//! collapsing -- because admission never re-serializes the candidate, only
//! ever publishing its original bytes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    registry: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-compiler-facts-lossless-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let registry = std::env::temp_dir().join(format!(
            "devscout-compiler-facts-lossless-registry-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let fixture = Self { root, registry };
        let init = fixture.run(&["init", "--no-hooks", "--no-map"]);
        assert!(init.status.success(), "init failed: {init:?}");
        fixture
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .current_dir(&self.root)
            .env("SCOUT_REGISTRY", &self.registry)
            .args(args)
            .output()
            .unwrap()
    }

    fn artifact_path(&self) -> PathBuf {
        self.root.join(".scout/graph/compiler-facts-v1.json")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_file(&self.registry);
    }
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/compiler-facts")
        .join(name)
}

#[test]
fn round_trip_preserves_every_promised_fact_byte_for_byte() {
    let fx = Fixture::new();
    let candidate_path = fixture_path("candidate.json");
    let original = fs::read(&candidate_path).unwrap();

    let out = fx.run(&["compiler-facts", "import", candidate_path.to_str().unwrap()]);
    assert!(out.status.success(), "{out:?}");

    let published = fs::read(fx.artifact_path()).unwrap();
    assert_eq!(
        published, original,
        "admission never re-serializes the candidate; the published artifact is the exact input bytes"
    );

    let read_back: Value = serde_json::from_slice(&published).unwrap();
    let symbols = read_back["symbols"].as_array().unwrap();

    // Two same-line occurrences: `Widget.Render` and `Gadget.Render` at the
    // same file and line.
    let same_line: Vec<_> = symbols
        .iter()
        .filter(|s| s["file"] == "Api/Widgets/Widget.cs" && s["line"] == 12)
        .collect();
    assert_eq!(same_line.len(), 3, "expected three occurrences on line 12");

    // Two overloads of one name.
    let render_overloads: Vec<_> = symbols
        .iter()
        .filter(|s| s["type"] == "Api.Widgets.Widget" && s["member"] == "Render")
        .map(|s| s["overloadSignature"].as_str().unwrap())
        .collect();
    assert!(render_overloads.contains(&"()->void"));
    assert!(render_overloads.contains(&"(bool)->void"));

    // A failed binding and an unresolved site.
    let failed = symbols
        .iter()
        .find(|s| s["member"] == "Load")
        .expect("failed binding present");
    assert_eq!(failed["bindingFailed"], true);
    assert_eq!(failed["diagnostic"], "CS1061");

    let unresolved = symbols
        .iter()
        .find(|s| s["member"] == "Bind")
        .expect("unresolved site present");
    assert_eq!(unresolved["unresolved"], true);

    // Compiler diagnostics survive.
    let diagnostics = read_back["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "CS0219");

    // The compilation-context health envelope survives, embedded verbatim
    // under its own version literal -- admission never parses its
    // internal shape.
    assert_eq!(read_back["context"]["schemaVersion"], 1);
    assert_eq!(read_back["context"]["envelope"]["state"], "complete");

    // Occurrence spans carry their declared encoding convention.
    for symbol in symbols {
        assert_eq!(symbol["spanEncoding"], "utf16-code-unit");
    }
}
