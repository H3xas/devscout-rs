//! Shape test for the committed compiler-facts snapshot
//! `fixtures/csharp-compiler-facts/compiler-facts.json`.
//!
//! The snapshot is produced by the engine's `--emit compiler-facts` mode
//! running over the fixture project in the same directory; CI's
//! `semantic-audit` job regenerates it and diffs the bytes. This file never
//! invokes `dotnet`: it reads the committed document and pins the shape the
//! Rust admission path relies on, so an engine-side drift fails `cargo
//! test` even on a machine with no .NET toolchain.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn snapshot_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-compiler-facts/compiler-facts.json")
}

fn text() -> String {
    let text = fs::read_to_string(snapshot_path()).expect("read compiler-facts snapshot");
    assert!(
        text.ends_with('\n') && !text.contains('\r'),
        "snapshot is LF-terminated"
    );
    text
}

fn load() -> Value {
    serde_json::from_str::<Value>(&text()).expect("snapshot parses as JSON")
}

#[test]
fn header_identity_matches_the_rust_admission_path_s_own_constants() {
    let doc = load();
    assert_eq!(doc["format"], "compiler-facts");
    assert_eq!(doc["contractVersion"], 1);
    assert_eq!(doc["artifactSchemaVersion"], 2);
    assert_eq!(doc["producer"]["name"], "scout-semantic");
    assert_eq!(doc["producer"]["engineRevision"], "2");
    assert_eq!(doc["profile"]["target"], "net9.0");
    assert_eq!(doc["profile"]["configuration"], "Debug");
    assert_eq!(doc["profile"]["platform"], "AnyCPU");
    assert_eq!(
        doc["dependencyFingerprint"],
        "1b08b298ead60b49666b3bfa8d389386770d87dc150a9b1eced58896652f3d43",
        "must match the sha256 of tools/scout-semantic/packages.lock.json"
    );
    assert_eq!(doc["context"]["schemaVersion"], 1);
    let context_fingerprint = doc["context"]["contextFingerprint"]
        .as_str()
        .expect("contextFingerprint is a string");
    assert_eq!(
        context_fingerprint.len(),
        64,
        "the derived context summary is a lower-case hex SHA-256 digest"
    );
    let compilations = doc["context"]["envelope"]["compilations"]
        .as_array()
        .expect("context.envelope.compilations is an array -- the real embedded envelope,                  not the frozen placeholder this delta replaces");
    assert!(!compilations.is_empty(), "the fixture project is one real compilation");
    for compilation in compilations {
        assert!(compilation.get("identity").is_some());
        assert!(compilation.get("fingerprint").is_some());
    }
    assert!(
        doc.get("sourceSnapshot").is_none(),
        "generated with --no-git: no source-snapshot identity is stamped"
    );
    assert_eq!(doc["completion"]["terminal"], true);
}

#[test]
fn the_deliberately_failing_unit_demotes_coverage_with_the_compiler_s_own_reason() {
    let doc = load();
    assert_eq!(
        doc["units"]["processed"],
        serde_json::json!(["Fixture|net9.0"])
    );
    assert_eq!(doc["units"]["missing"], serde_json::json!([]));
    assert_eq!(doc["coverage"]["state"], "incomplete");
    let incomplete = doc["coverage"]["incompleteUnits"].as_array().unwrap();
    assert_eq!(incomplete.len(), 1);
    assert_eq!(incomplete[0]["unit"], "Fixture|net9.0");
    assert!(incomplete[0]["reason"].as_str().unwrap().contains("CS1061"));
}

#[test]
fn every_promised_fact_shape_is_present() {
    let doc = load();
    let diagnostics = doc["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics.iter().any(|d| d["code"] == "CS1061"),
        "failed binding"
    );
    assert!(
        diagnostics.iter().any(|d| d["code"] == "CS0103"),
        "unresolved site"
    );

    let symbols = doc["symbols"].as_array().unwrap();
    let same_line: Vec<_> = symbols
        .iter()
        .filter(|s| s["file"] == "Widgets.cs" && s["line"] == 5)
        .collect();
    assert_eq!(same_line.len(), 2, "two same-line occurrences");
    let render_overloads: Vec<_> = symbols
        .iter()
        .filter(|s| s["member"] == "Render")
        .map(|s| s["overloadSignature"].as_str().unwrap())
        .collect();
    assert!(render_overloads.contains(&"()->void"));
    assert!(render_overloads.contains(&"(bool)->void"));

    // A qualified sibling unrelated to the failing unit is retained.
    assert!(symbols
        .iter()
        .any(|s| s["type"] == "CompilerFacts.Widgets.Gadget" && s["member"] == "Ping"));

    for symbol in symbols {
        assert_eq!(symbol["spanEncoding"], "utf16-code-unit");
        assert!(symbol.get("genericArity").is_some());
        assert!(symbol.get("overloadSignature").is_some());
    }
}

#[test]
fn symbols_are_sorted_by_file_then_line_then_type_then_member() {
    let doc = load();
    let symbols = doc["symbols"].as_array().unwrap();
    let key = |s: &Value| {
        (
            s["file"].as_str().unwrap().to_owned(),
            s["line"].as_u64().unwrap(),
            s["type"].as_str().unwrap().to_owned(),
            s["member"].as_str().unwrap_or("").to_owned(),
            s["overloadSignature"].as_str().unwrap().to_owned(),
        )
    };
    for pair in symbols.windows(2) {
        let (a, b) = (key(&pair[0]), key(&pair[1]));
        assert!(a <= b, "symbols are sorted:\n{a:?}\n{b:?}");
    }
}
