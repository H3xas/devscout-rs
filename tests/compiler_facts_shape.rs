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
        .expect(
            "context.envelope.compilations is an array -- the real embedded envelope, \
             not the frozen placeholder this delta replaces",
        );
    assert!(
        !compilations.is_empty(),
        "the fixture project is one real compilation"
    );
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

#[test]
fn occurrence_sites_are_present_with_the_stated_span_and_identity_encoding() {
    let doc = load();
    assert_eq!(
        doc["occurrences"]["spanEncoding"],
        "utf16-code-unit-line1-char0-end-exclusive"
    );
    assert_eq!(
        doc["occurrences"]["identityEncoding"],
        "fully-qualified-display-format"
    );
    let sites = doc["occurrences"]["sites"].as_array().unwrap();
    assert!(!sites.is_empty());
    for site in sites {
        for key in [
            "file",
            "shape",
            "span",
            "name",
            "caller",
            "resolution",
            "candidateReason",
            "target",
            "candidates",
            "compilation",
            "documentContentIdentity",
            "targetDocumentContentIdentities",
        ] {
            assert!(
                site.get(key).is_some(),
                "occurrence missing `{key}`: {site}"
            );
        }
        let compilation = &site["compilation"];
        assert!(compilation.get("identity").is_some());
        assert!(compilation.get("fingerprint").is_some());
    }
}

#[test]
fn two_same_line_call_sites_are_distinct_records_with_distinct_spans() {
    let doc = load();
    let sites = doc["occurrences"]["sites"].as_array().unwrap();
    let same_line: Vec<_> = sites
        .iter()
        .filter(|s| s["file"] == "Callers.cs" && s["span"]["startLine"] == 8)
        .collect();
    assert_eq!(
        same_line.len(),
        2,
        "two same-line occurrences, not deduplicated"
    );
    assert_ne!(same_line[0]["span"], same_line[1]["span"]);
    assert_ne!(same_line[0]["name"], same_line[1]["name"]);
    let overloads: Vec<_> = same_line
        .iter()
        .map(|s| s["target"]["overloadSignature"].as_str().unwrap())
        .collect();
    assert!(overloads.contains(&"()->void"));
    assert!(overloads.contains(&"(bool)->void"));
}

#[test]
fn two_same_line_occurrences_of_one_target_are_not_deduplicated() {
    let doc = load();
    let sites = doc["occurrences"]["sites"].as_array().unwrap();
    let same_line_assist: Vec<_> = sites
        .iter()
        .filter(|s| s["file"] == "Callers.cs" && s["span"]["startLine"] == 11)
        .collect();
    assert_eq!(
        same_line_assist.len(),
        2,
        "two same-line occurrences of the exact same target must both be kept, \
         proving there is no (file, line, target) dedup key"
    );
    assert_ne!(same_line_assist[0]["span"], same_line_assist[1]["span"]);
    for site in &same_line_assist {
        assert_eq!(site["target"]["type"], "CompilerFacts.Widgets.Helper");
        assert_eq!(site["target"]["member"], "Assist");
    }
}

#[test]
fn the_generic_nested_caller_s_identity_matches_the_declared_symbol_facts_own_encoding() {
    let doc = load();
    let symbols = doc["symbols"].as_array().unwrap();
    let nested_go_symbol = symbols
        .iter()
        .find(|s| s["type"] == "CompilerFacts.Widgets.Callers.Nested<T>" && s["member"] == "Go")
        .expect("the declared-symbol fact for the generic, nested Nested<T>.Go");

    let sites = doc["occurrences"]["sites"].as_array().unwrap();
    let nested_caller_site = sites
        .iter()
        .find(|s| {
            s["caller"]["member"] == "Go"
                && s["caller"]["type"] == "CompilerFacts.Widgets.Callers.Nested<T>"
        })
        .expect("an occurrence called from the generic, nested Nested<T>.Go");

    assert_eq!(
        nested_caller_site["caller"]["type"], nested_go_symbol["type"],
        "an occurrence's caller.type for a nested, generic type must read identically to \
         symbols[].type for the same type -- not devscout's own Outer+Inner graph def-id form"
    );
    assert_eq!(
        nested_caller_site["caller"]["assembly"],
        nested_go_symbol["assembly"]
    );
    assert_eq!(
        nested_caller_site["caller"]["genericArity"], 0,
        "Go itself declares no type parameters, even though its enclosing type does"
    );
    assert_eq!(
        nested_caller_site["caller"]["overloadSignature"],
        "()->void"
    );
}

#[test]
fn a_cross_document_target_s_content_identity_names_a_document_other_than_the_caller_s_own() {
    let doc = load();
    let sites = doc["occurrences"]["sites"].as_array().unwrap();
    let cross_document_site = sites
        .iter()
        .find(|s| {
            s["file"] == "Callers.cs"
                && s["target"]["type"] == "CompilerFacts.Widgets.Helper"
                && s["target"]["member"] == "Assist"
        })
        .expect("a call whose target (Helper.Assist) is declared in Other.cs, not Callers.cs");

    let own_identity = cross_document_site["documentContentIdentity"]
        .as_str()
        .expect("documentContentIdentity is a string");
    let target_identities = cross_document_site["targetDocumentContentIdentities"]
        .as_array()
        .expect("targetDocumentContentIdentities is an array");
    assert_eq!(
        target_identities.len(),
        1,
        "Helper.Assist has exactly one declaring document"
    );
    let target_identity = target_identities[0]
        .as_str()
        .expect("target document identity is a string");

    assert!(target_identity.starts_with("sha1:"));
    assert_ne!(
        target_identity, own_identity,
        "the target's own declaring document (Other.cs) must carry a content identity distinct \
         from the calling document's (Callers.cs), proving the identity names the target's \
         document rather than repeating the caller's"
    );
}

#[test]
fn span_and_name_coordinates_match_the_fixture_source_by_hand_count() {
    // Line 8 of Callers.cs, hand-counted against the fixture's own source
    // text (not trusting the producer's own word for the convention):
    //   "        widget.Render(); widget.Render(true);"
    //    0123456789...
    // `widget.Render` (the member-access node's own span) starts at column
    // 8; the bound name token `Render` starts at column 15 -- both 0-based,
    // UTF-16 code units, end exclusive.
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-compiler-facts/Callers.cs"),
    )
    .unwrap();
    let line8 = source.lines().nth(7).unwrap(); // 1-based line 8, 0-indexed nth
    assert!(line8.starts_with("        widget.Render();"));
    let widget_dot_render_start = line8.find("widget.Render").unwrap();
    let render_start = line8.find("Render").unwrap();
    assert_eq!(widget_dot_render_start, 8);
    assert_eq!(render_start, 15);

    let doc = load();
    let sites = doc["occurrences"]["sites"].as_array().unwrap();
    let first = sites
        .iter()
        .find(|s| {
            s["file"] == "Callers.cs"
                && s["span"]["startLine"] == 8
                && s["target"]["overloadSignature"] == "()->void"
        })
        .expect("the zero-arg Render() call site on line 8");
    assert_eq!(first["span"]["startChar"], 8);
    assert_eq!(
        first["span"]["endChar"], 21,
        "member-access span ends before the invocation's own parens"
    );
    assert_eq!(
        first["name"]["char"], 15,
        "the bound name token's own start, not the whole span's"
    );
}

#[test]
fn every_resolution_state_is_represented_and_the_failing_site_is_present() {
    let doc = load();
    let sites = doc["occurrences"]["sites"].as_array().unwrap();
    for state in [
        "confirmed",
        "ambiguous",
        "unresolved",
        "inaccessible",
        "dynamic",
    ] {
        assert!(
            sites.iter().any(|s| s["resolution"] == state),
            "no occurrence in resolution state `{state}`"
        );
    }

    // The control this AC exists for: the oracle's own refs.jsonl
    // deliberately drops a site whose symbol and candidates are both empty;
    // this producer's own unresolved site must still be present.
    let unresolved: Vec<_> = sites
        .iter()
        .filter(|s| s["resolution"] == "unresolved")
        .collect();
    assert!(
        unresolved
            .iter()
            .any(|s| s["candidates"].as_array().unwrap().is_empty() && s["target"].is_null()),
        "an unresolved site must carry an empty candidate set and a null target, not be dropped"
    );

    let ambiguous = sites
        .iter()
        .find(|s| s["file"] == "Callers.cs" && s["resolution"] == "ambiguous")
        .unwrap();
    assert!(ambiguous["candidates"].as_array().unwrap().len() >= 2);
    assert_eq!(ambiguous["candidateReason"], "OverloadResolutionFailure");

    let inaccessible = sites
        .iter()
        .find(|s| s["resolution"] == "inaccessible")
        .unwrap();
    assert_eq!(inaccessible["candidateReason"], "Inaccessible");
    assert_eq!(inaccessible["candidates"].as_array().unwrap().len(), 1);

    let dynamic = sites.iter().find(|s| s["resolution"] == "dynamic").unwrap();
    assert_eq!(dynamic["candidateReason"], "LateBound");
}
