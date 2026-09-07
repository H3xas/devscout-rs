//! CLI coverage for an ambiguous TYPE seed answered under `--json`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

// `First.Owner` and `Second.Owner` carry the same simple name, so the bare seed
// `Owner` resolves to neither and the verb must list both.
const SEED: &str = "Owner";

struct Fixture {
    root: PathBuf,
    registry: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-json-ambiguous-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-nested-resolution");
        fs::copy(source.join("Collisions.cs"), root.join("Collisions.cs")).unwrap();
        let registry = root.join("registry.json");
        let fixture = Self { root, registry };
        let init = fixture.run(&["init", "--no-hooks"]);
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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_ambiguous_json(out: &Output) {
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let body = String::from_utf8(out.stdout.clone()).unwrap();
    let body = body.trim_end_matches('\n');
    assert!(
        body.starts_with("{\"schema_version\":1,"),
        "schema_version leads every answer this contract covers: {body}"
    );
    let value: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(value["outcome"], "ambiguous");
    assert_eq!(value["query"], SEED);
    let candidates = value["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2, "{body}");
    assert_eq!(candidates[0]["id"], "First.Owner");
    assert_eq!(candidates[0]["file"], "Collisions.cs");
    assert_eq!(candidates[0]["line"], 3);
    assert_eq!(candidates[0]["kind"], "class");
    assert_eq!(candidates[1]["id"], "Second.Owner");
    assert_eq!(candidates[1]["file"], "Collisions.cs");
    assert_eq!(candidates[1]["line"], 11);
    assert_eq!(candidates[1]["kind"], "class");
}

#[test]
fn every_verb_answers_an_ambiguous_type_seed_as_json_under_json() {
    let fixture = Fixture::new();
    for verb in ["refs", "read", "impact", "tests"] {
        let out = fixture.run(&[verb, SEED, "--json"]);
        assert_ambiguous_json(&out);
    }
}

#[test]
fn the_text_candidate_list_is_what_it_was_without_the_flag() {
    let fixture = Fixture::new();
    let want = "ambiguous symbol \"Owner\" — 2 candidates:\nFirst.Owner  Collisions.cs:3  class\nSecond.Owner  Collisions.cs:11  class\n";
    for verb in ["refs", "read", "impact", "tests"] {
        let out = fixture.run(&[verb, SEED]);
        assert_eq!(out.status.code(), Some(1), "{verb}: {out:?}");
        assert_eq!(String::from_utf8(out.stdout).unwrap(), want, "{verb}");
    }
}
