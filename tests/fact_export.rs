//! Acceptance tests for `devscout export` over the three dedicated fixture
//! trees. Expectations are read off the private artifacts the index wrote, so
//! an assertion compares the contract against the index rather than against a
//! second copy of the same claim.

#[path = "fact_export_support/mod.rs"]
mod support;

use std::collections::BTreeMap;
use std::fs;

use serde_json::Value;
use support::{
    facts_of, families_of, family_lines, header_of, normalize, oracle_counts, pin_path, schema,
    schema_subset_violation, trailer_of, validate, Fixture, LANGUAGES,
};

fn expected_test_files(language: &str) -> Vec<(&'static str, &'static str)> {
    match language {
        "csharp" => vec![
            ("tests/LedgerHarness.cs", "project"),
            ("tests/LedgerTests.cs", "attribute"),
        ],
        "typescript" => vec![("src/page.spec.ts", "path")],
        "rust" => vec![("src/lib.rs", "attribute")],
        "transport" => vec![("tests/HarnessTests.cs", "attribute")],
        other => panic!("no expectation for {other}"),
    }
}

#[test]
fn fact_export_ac_1_the_stream_is_a_header_fact_lines_and_a_trailer() {
    for language in LANGUAGES {
        let fixture = Fixture::build(language);
        let lines = fixture.lines();
        assert!(
            lines.len() >= 3,
            "{language}: a stream has at least three lines"
        );

        let header = header_of(&lines);
        assert_eq!(header["format"], "devscout-facts", "{language}");
        assert_eq!(header["schemaVersion"], 1, "{language}");
        assert!(header["provenance"].is_object(), "{language}");
        assert!(header["coverage"].is_object(), "{language}");
        assert!(header["freshness"]["state"].is_string(), "{language}");

        let facts = facts_of(&lines);
        assert!(!facts.is_empty(), "{language}: the fixtures record facts");
        for fact in &facts {
            assert!(fact["family"].is_string(), "{language}: {fact}");
            assert!(fact["evidence"].is_string(), "{language}: {fact}");
        }

        let trailer = trailer_of(&lines);
        assert_eq!(trailer["trailer"], "devscout-facts", "{language}");
        assert_eq!(
            trailer["facts"].as_u64().expect("a count") as usize,
            facts.len(),
            "{language}: the trailer counts the facts it followed"
        );
        let observed = families_of(&facts);
        for (family, count) in trailer["counts"].as_object().expect("counts") {
            let seen = observed.get(family).copied().unwrap_or(0);
            assert_eq!(
                count.as_u64().expect("a count") as usize,
                seen,
                "{language}/{family}"
            );
        }
    }
}

#[test]
fn fact_export_ac_1_out_writes_the_stdout_bytes_through_a_temporary_file() {
    let fixture = Fixture::build("csharp");
    let stdout = fixture.export(&[]);
    let target = fixture.root.join("stream.ndjson");
    let out = fixture.run(&["export", "--out", target.to_str().expect("utf-8 path")]);
    assert!(out.status.success(), "export --out: {out:?}");
    assert!(
        out.stdout.is_empty(),
        "--out writes the stream to the file, not stdout"
    );
    let written = fs::read(&target).expect("read the written stream");
    assert_eq!(written, stdout, "--out writes the same bytes as stdout");
    let leftover: Vec<String> = fs::read_dir(&fixture.root)
        .expect("read the fixture root")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.contains(".tmp."))
        .collect();
    assert!(
        leftover.is_empty(),
        "a successful run leaves no temp file: {leftover:?}"
    );
}

#[test]
fn fact_export_ac_1_a_failed_out_leaves_no_file_and_keeps_an_existing_one() {
    let fixture = Fixture::build("csharp");

    let missing_dir = fixture.root.join("nowhere/stream.ndjson");
    let out = fixture.run(&["export", "--out", missing_dir.to_str().expect("utf-8 path")]);
    assert!(!out.status.success(), "a missing directory is not created");
    assert!(
        !missing_dir.exists(),
        "no file appears at the requested path"
    );

    let onto_a_directory = fixture.root.join("target-dir");
    fs::create_dir(&onto_a_directory).expect("create the target directory");
    let out = fixture.run(&[
        "export",
        "--out",
        onto_a_directory.to_str().expect("utf-8 path"),
    ]);
    assert!(
        !out.status.success(),
        "a directory is not replaced by a stream"
    );
    assert!(onto_a_directory.is_dir(), "the directory is still there");

    let kept = fixture.root.join("kept.ndjson");
    fs::write(&kept, b"previous\n").expect("write the pre-existing file");
    let out = fixture.run(&[
        "export",
        "--out",
        fixture
            .root
            .join("nowhere/again.ndjson")
            .to_str()
            .expect("utf-8 path"),
    ]);
    assert!(!out.status.success());
    assert_eq!(
        fs::read(&kept).expect("read the kept file"),
        b"previous\n",
        "a file already at another path is untouched"
    );
    let mut stray: Vec<String> = Vec::new();
    for dir in [&fixture.root, &onto_a_directory] {
        for entry in fs::read_dir(dir).expect("read dir").flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains(".tmp.") {
                stray.push(name);
            }
        }
    }
    assert!(
        stray.is_empty(),
        "no temporary file survives a failure: {stray:?}"
    );
}

#[test]
fn fact_export_ac_1_a_malformed_invocation_exits_two_with_usage_and_no_stream() {
    let fixture = Fixture::build("csharp");
    let cases: Vec<Vec<&str>> = vec![
        vec!["export", "extra"],
        vec!["export", "--unknown"],
        vec!["export", "--out"],
        vec!["export", "--out", "--json"],
        vec!["export", "--out", "a.ndjson", "--out", "b.ndjson"],
    ];
    for args in cases {
        let out = fixture.run(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        assert!(out.stdout.is_empty(), "{args:?} writes no stream");
        let stderr = String::from_utf8(out.stderr).expect("stderr is UTF-8");
        assert!(
            stderr.contains("usage: devscout export [--out FILE]"),
            "{args:?} prints the usage line: {stderr}"
        );
        assert!(!fixture.root.join("a.ndjson").exists());
    }
}

#[test]
fn fact_export_ac_2_the_counts_are_the_facts_the_index_records() {
    for language in LANGUAGES {
        let fixture = Fixture::build(language);
        let lines = fixture.lines();
        let facts = facts_of(&lines);
        let observed = families_of(&facts);
        let oracle = oracle_counts(&fixture);
        for (family, expected) in &oracle {
            assert_eq!(
                observed.get(family).copied().unwrap_or(0),
                *expected,
                "{language}/{family}: the stream carries what the index records"
            );
        }
        let trailer = trailer_of(&lines);
        for (family, expected) in &oracle {
            assert_eq!(
                trailer["counts"][family].as_u64().expect("a count") as usize,
                *expected,
                "{language}/{family}"
            );
        }
    }
}

#[test]
fn fact_export_ac_2_two_calls_on_one_line_export_as_two_lines() {
    let fixture = Fixture::build("csharp");
    let lines = fixture.lines();
    let doubled: Vec<&String> = lines
        .iter()
        .filter(|line| {
            line.contains("\"kind\":\"uses-member\"") && line.contains("\"member\":\"Now\"")
        })
        .collect();
    assert_eq!(
        doubled.len(),
        2,
        "one call site is two occurrences: {doubled:?}"
    );
    assert_eq!(doubled[0], doubled[1], "the two lines are identical");
    let adjacent = lines
        .windows(2)
        .any(|pair| pair[0] == pair[1] && pair[0].as_str() == doubled[0].as_str());
    assert!(
        adjacent,
        "identical lines stay adjacent rather than being deduplicated"
    );
}

#[test]
fn fact_export_ac_2_the_test_classification_matches_the_fixture() {
    for language in LANGUAGES {
        let fixture = Fixture::build(language);
        let lines = fixture.lines();
        let mut seen: Vec<(String, String)> = family_lines(&lines, "test-file")
            .iter()
            .map(|fact| {
                (
                    fact["file"].as_str().expect("a file").to_string(),
                    fact["via"].as_str().expect("a vouch").to_string(),
                )
            })
            .collect();
        seen.sort();
        let expected: Vec<(String, String)> = expected_test_files(language)
            .into_iter()
            .map(|(file, via)| (file.to_string(), via.to_string()))
            .collect();
        assert_eq!(
            seen, expected,
            "{language}: the test-file set and its vouches"
        );
    }
}

#[test]
fn fact_export_ac_2_imported_edges_are_not_exported() {
    let fixture = Fixture::build("csharp");
    let before = fixture.lines();
    let imported =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/import-edges/export.json");
    let out = fixture.run(&[
        "import-edges",
        imported.to_str().expect("utf-8 path"),
        "--repo",
        "ledger",
    ]);
    assert!(out.status.success(), "import-edges failed: {out:?}");
    let after = fixture.lines();
    assert_eq!(
        before, after,
        "one repository per export, no imported edges"
    );
}

#[test]
fn fact_export_ac_3_an_extracted_family_with_no_facts_reads_extracted() {
    let fixture = Fixture::build("typescript");
    let lines = fixture.lines();
    let header = header_of(&lines);
    let trailer = trailer_of(&lines);
    assert_eq!(
        trailer["counts"]["publish-site"], 0,
        "this tree records no C# publish site"
    );
    assert_eq!(
        header["coverage"]["publish-site"]["csharp"], "extracted",
        "the C# family is extracted by THIS release even here"
    );
    assert_eq!(
        header["coverage"]["http-route"]["csharp"], "not-extracted",
        "a family this release does not extract reads differently"
    );
}

#[test]
fn fact_export_ac_3_every_evidence_word_is_one_the_schema_declares() {
    let root = schema();
    let declared: Vec<String> = root["$defs"]["edgeEvidence"]["enum"]
        .as_array()
        .expect("the edge evidence enum")
        .iter()
        .map(|word| word.as_str().expect("a word").to_string())
        .collect();
    for language in LANGUAGES {
        let fixture = Fixture::build(language);
        let lines = fixture.lines();
        for fact in facts_of(&lines) {
            let evidence = fact["evidence"].as_str().expect("a word");
            if fact["family"] == "edge" {
                assert!(
                    declared.contains(&evidence.to_string()),
                    "{language}: {evidence}"
                );
            }
        }
        let guess = family_lines(&lines, "edge")
            .into_iter()
            .filter(|fact| fact["tier"] == "guess")
            .count();
        let stated = family_lines(&lines, "edge")
            .into_iter()
            .filter(|fact| fact["evidence"] == "heuristic-guess")
            .count();
        assert_eq!(guess, stated, "{language}: a guess is exported as a guess");
    }
}

#[test]
fn fact_export_ac_4_provenance_separates_source_analysis_and_index() {
    for language in LANGUAGES {
        let fixture = Fixture::build(language);
        let lines = fixture.lines();
        let header = header_of(&lines);
        let provenance = &header["provenance"];
        let record = fixture.record();
        let graph = fixture.graph();

        assert_eq!(
            provenance["source"]["indexedHead"], record["indexed_head"],
            "{language}"
        );
        assert_eq!(provenance["source"]["scope"], record["scope"], "{language}");
        assert_eq!(
            provenance["source"]["fileCount"], record["file_count"],
            "{language}"
        );
        assert_eq!(
            provenance["analysis"]["graphSchemaVersion"], graph["schema_version"],
            "{language}"
        );
        assert_eq!(
            provenance["analysis"]["devscoutVersion"],
            env!("CARGO_PKG_VERSION"),
            "{language}"
        );
        assert_eq!(
            provenance["analysis"]["fragmentGeneration"],
            cache_generation(&fixture),
            "{language}"
        );
        assert_eq!(
            provenance["analysis"]["compilerFacts"], "none",
            "{language}"
        );
        assert_eq!(provenance["analysis"]["semanticLane"], "none", "{language}");
        assert_eq!(
            provenance["index"]["generation"], record["generation"],
            "{language}"
        );
        assert_eq!(
            provenance["index"]["indexedAt"], record["indexed_at"],
            "{language}"
        );

        let config: Vec<Value> = fixture.record()["config_inputs"]
            .as_object()
            .expect("config inputs")
            .iter()
            .map(|(path, digest)| {
                serde_json::json!({
                    "path": path,
                    "digest": format!("sha256:{}", digest.as_str().expect("a bare digest")),
                })
            })
            .collect();
        assert_eq!(
            provenance["analysis"]["configInputs"],
            Value::Array(config),
            "{language}: configuration digests, sorted by path"
        );

        let pairs: Vec<Value> = fixture.digests()["files"]
            .as_object()
            .expect("the digest table")
            .iter()
            .map(|(path, digest)| {
                serde_json::json!([
                    path,
                    format!("sha256:{}", digest.as_str().expect("a bare digest")),
                ])
            })
            .collect();
        let table = serde_json::to_string(&pairs).expect("serialize the pairs");
        assert_eq!(
            provenance["source"]["fileDigests"],
            format!("sha256:{}", support::sha256(table.as_bytes())),
            "{language}: the digest of the digest table"
        );

        let encoded = serde_json::to_string(&header).expect("serialize the header");
        assert!(
            !encoded.contains(fixture.root.to_str().expect("utf-8 root")),
            "{language}: no absolute path reaches the stream"
        );
        assert!(
            !encoded.contains("owner"),
            "{language}: the record's owner is never written"
        );
    }
}

fn cache_generation(fixture: &Fixture) -> u64 {
    let name = fixture
        .cache_path()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a cache file name")
        .to_string();
    name.trim_start_matches("fragments-v")
        .trim_end_matches(".json")
        .parse()
        .expect("the cache name carries its generation")
}

#[test]
fn fact_export_ac_4_an_edited_tree_changes_the_payload_digest_and_a_remap_keeps_it() {
    let fixture = Fixture::build("rust");
    let first = trailer_of(&fixture.lines())["payloadDigest"]
        .as_str()
        .expect("a digest")
        .to_string();

    let source = fixture.root.join("src/shelf.rs");
    let edited = format!(
        "{}\n/// How deep the shelf is.\npub fn depth() -> u32 {{\n    400\n}}\n",
        fs::read_to_string(&source).expect("read the fixture source")
    );
    fs::write(&source, edited).expect("edit the fixture source");
    let stale = fixture.lines();
    assert_eq!(
        trailer_of(&stale)["payloadDigest"]
            .as_str()
            .expect("a digest"),
        first,
        "an unmapped edit is not in the facts: freshness changes, the payload does not"
    );
    assert_eq!(
        header_of(&stale)["freshness"]["state"],
        "stale",
        "the stale index is labelled, not refused"
    );

    fixture.map();
    let remapped = fixture.lines();
    assert_ne!(
        trailer_of(&remapped)["payloadDigest"]
            .as_str()
            .expect("a digest"),
        first,
        "one edited fact changes the payload digest"
    );
    assert_eq!(header_of(&remapped)["freshness"]["state"], "fresh");
}

#[test]
fn fact_export_ac_4_a_remap_of_an_unchanged_tree_keeps_the_payload_digest() {
    let fixture = Fixture::build("typescript");
    let first = fixture.lines();
    fixture.map();
    let second = fixture.lines();
    assert_eq!(
        trailer_of(&first)["payloadDigest"],
        trailer_of(&second)["payloadDigest"],
        "a re-map of an unchanged tree records the same facts"
    );
    assert_eq!(
        facts_of(&first),
        facts_of(&second),
        "and the same lines, in the same order"
    );
}

#[test]
fn fact_export_ac_5_two_exports_of_one_index_are_byte_identical() {
    let fixture = Fixture::build("csharp");
    assert_eq!(
        fixture.export(&[]),
        fixture.export(&[]),
        "an unchanged index renders one stream, not two"
    );
    let header = String::from_utf8(fixture.export(&[])).expect("UTF-8");
    let header = header.lines().next().expect("a header").to_string();
    for key in [
        "exportedAt",
        "exportTime",
        "generatedAt",
        "timestamp",
        "now",
    ] {
        assert!(
            !header.contains(key),
            "no export-time clock reaches the header: {key}"
        );
    }
}

#[test]
fn fact_export_ac_5_the_fact_lines_follow_the_canonical_order() {
    let fixture = Fixture::build("csharp");
    let lines = fixture.lines();
    let facts: Vec<&String> = lines[1..lines.len() - 1].iter().collect();
    let rank = |line: &str| -> (usize, String, u64, String) {
        let fact: Value = serde_json::from_str(line).expect("a fact line parses");
        let family = fact["family"].as_str().expect("a family").to_string();
        let order = [
            "definition",
            "edge",
            "publish-site",
            "handler-registration",
            "di-registration",
            "unresolved-import",
            "test-file",
        ];
        (
            order
                .iter()
                .position(|name| *name == family)
                .expect("a ranked family"),
            fact["file"].as_str().unwrap_or_default().to_string(),
            fact["line"].as_u64().unwrap_or(0),
            line.to_string(),
        )
    };
    let mut sorted = facts.clone();
    sorted.sort_by_key(|line| rank(line));
    assert_eq!(
        facts, sorted,
        "the stream's own order is the documented one"
    );
}

#[test]
fn fact_export_ac_7_every_pinned_line_validates_against_the_schema() {
    let root = schema();
    assert_eq!(
        schema_subset_violation(&root),
        None,
        "the schema uses only the documented keyword subset"
    );
    for language in LANGUAGES {
        let pin = pin_path(language);
        let text =
            fs::read_to_string(&pin).unwrap_or_else(|e| panic!("read {}: {e}", pin.display()));
        for (index, line) in text.lines().enumerate() {
            let value: Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("{language} line {index}: {e}"));
            validate(&value, &root, &root)
                .unwrap_or_else(|e| panic!("{language} line {index}: {e}\n{line}"));
        }
    }
}

#[test]
fn fact_export_ac_7_the_pins_are_regenerated_byte_for_byte_or_blessed() {
    for language in LANGUAGES {
        let fixture = Fixture::build(language);
        let generated = normalize(&fixture.export(&[]));
        let pin = pin_path(language);
        if std::env::var("BLESS").as_deref() == Ok("1") {
            fs::write(&pin, &generated).expect("write the pin");
            continue;
        }
        let committed = fs::read_to_string(&pin).unwrap_or_default();
        assert_eq!(
            generated, committed,
            "fixtures/fact-export/{language}.ndjson is stale -- regenerate with `BLESS=1 cargo test --test fact_export`"
        );
    }
}

#[test]
fn fact_export_ac_7_a_pin_normalizes_only_the_per_run_header_values() {
    for language in LANGUAGES {
        let fixture = Fixture::build(language);
        let raw = fixture.lines();
        let pin = fs::read_to_string(pin_path(language)).expect("read the pin");
        let pinned: Vec<String> = pin.lines().map(str::to_string).collect();
        assert_eq!(pinned[0].matches("devscout-facts").count(), 1, "{language}");
        assert_eq!(
            pinned.len(),
            raw.len(),
            "{language}: normalization never adds or drops a line"
        );
        assert_eq!(
            pinned[1..],
            raw[1..],
            "{language}: only the header is normalized"
        );
        let header = header_of(&pinned);
        assert_eq!(header["schemaVersion"], 1, "{language}");
        assert_eq!(
            header["provenance"]["analysis"]["devscoutVersion"],
            "devscoutVersion"
        );
        assert_eq!(
            header["provenance"]["analysis"]["fragmentGeneration"], 0,
            "{language}: the fragment generation a pin carries is a placeholder"
        );
        assert_eq!(
            header["provenance"]["analysis"]["graphSchemaVersion"],
            fixture.graph()["schema_version"],
            "{language}: the real schema version is asserted beside it"
        );
    }
}

#[test]
fn fact_export_ac_9_an_export_leaves_the_index_byte_identical() {
    let fixture = Fixture::build("csharp");
    let before = fixture.state_snapshot();
    fixture.export(&[]);
    assert_eq!(before, fixture.state_snapshot(), "an export writes nothing");

    let target = fixture.root.join("stream.ndjson");
    fixture.run(&["export", "--out", target.to_str().expect("utf-8 path")]);
    assert_eq!(before, fixture.state_snapshot(), "and neither does --out");
}

#[test]
fn fact_export_ac_9_a_refused_export_leaves_the_index_byte_identical() {
    let fixture = Fixture::build("csharp");
    let graph = fixture.state_path().join("graph/graph.json");
    let bytes = fs::read(&graph).expect("read the graph");
    fs::write(&graph, &bytes[..bytes.len() / 2]).expect("truncate the graph");
    let before = fixture.state_snapshot();
    let out = fixture.run(&["export"]);
    assert!(!out.status.success(), "a truncated graph refuses");
    assert_eq!(
        before,
        fixture.state_snapshot(),
        "a refusal changes nothing"
    );
}

#[test]
fn fact_export_ac_9_the_existing_answers_are_unchanged() {
    let fixture = Fixture::build("csharp");
    let queries: Vec<Vec<&str>> = vec![
        vec!["refs", "Ledger", "--json"],
        vec!["read", "Ledger", "--json"],
        vec!["impact", "src/Ledger.cs", "--json"],
        vec!["tests", "Ledger", "--json"],
    ];
    let mut before: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for query in &queries {
        let out = fixture.run(query);
        before.insert(query.join(" "), out.stdout);
    }
    fixture.export(&[]);
    for query in &queries {
        let out = fixture.run(query);
        assert_eq!(
            out.stdout,
            before[&query.join(" ")],
            "{query:?} answers the same bytes after an export"
        );
    }
}

#[test]
fn fact_export_ac_1_the_global_directory_option_exports_the_named_root() {
    let fixture = Fixture::build("rust");
    // Run from OUTSIDE the repository: only `-C` names the tree to export.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_devscout"))
        .args(["-C", fixture.root.to_str().expect("utf-8 path"), "export"])
        .current_dir(&fixture.home)
        .env("HOME", &fixture.home)
        .env("SCOUT_REGISTRY", fixture.home.join("repos.json"))
        .env("SCOUT_CONTENT_DB", fixture.home.join("content.db"))
        .output()
        .expect("devscout must run");
    assert!(
        out.status.success(),
        "-C export failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.stdout,
        fixture.export(&[]),
        "the named root exports exactly what the root itself would"
    );
}

#[test]
fn fact_export_ac_1_an_out_path_with_spaces_is_written_whole() {
    let fixture = Fixture::build("rust");
    let target = fixture.root.join("with space.ndjson");
    let out = fixture.run(&["export", "--out", target.to_str().expect("utf-8 path")]);
    assert!(out.status.success(), "export --out with a space: {out:?}");
    assert_eq!(
        fs::read(&target).expect("read the written stream"),
        fixture.export(&[]),
        "a path with a space is one argument, not two"
    );
}

#[test]
fn fact_export_ac_4_a_dirty_path_the_index_kept_no_digest_of_reads_null() {
    let fixture = Fixture::build("rust");
    let edited = fs::read_to_string(fixture.root.join("src/lib.rs")).expect("read the source");
    fs::write(
        fixture.root.join("src/lib.rs"),
        format!("{edited}\n// a change the map is about to see\n"),
    )
    .expect("edit a mapped file");
    fs::remove_file(fixture.root.join("src/shelf.rs")).expect("delete a mapped file");
    fixture.map();

    let lines = fixture.lines();
    let header = header_of(&lines);
    let dirty = header["provenance"]["source"]["dirtyAtMap"]
        .as_array()
        .expect("dirtyAtMap is an array")
        .clone();
    let recorded = dirty
        .iter()
        .find(|entry| entry["path"] == "src/lib.rs")
        .expect("the edited file was dirty when the map read it");
    let table = fixture.digests();
    assert_eq!(
        recorded["digest"],
        Value::String(format!(
            "sha256:{}",
            table["files"]["src/lib.rs"]
                .as_str()
                .expect("the digest table holds the edited file")
        )),
        "a path the map read gets its recorded content digest"
    );
    let deleted = dirty
        .iter()
        .find(|entry| entry["path"] == "src/shelf.rs")
        .expect("the deleted file was dirty when the map read it");
    assert!(
        deleted["digest"].is_null(),
        "a path whose bytes the index kept no digest of reads null, never a fabricated digest: {deleted}"
    );
}
