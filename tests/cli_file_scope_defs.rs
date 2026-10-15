//! File-scope TypeScript and JavaScript declaration behavior at the CLI boundary.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    base: PathBuf,
    repo: PathBuf,
    home: PathBuf,
    map_output: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            copy_tree(&path, &target.join(name));
        } else if name != "README.md" {
            fs::copy(path, target.join(name)).unwrap();
        }
    }
}

impl Fixture {
    fn build(source: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let base = env::temp_dir().join(format!("scout-file-scope-{}-{n}", std::process::id()));
        let repo = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&home).unwrap();
        copy_tree(&Path::new(env!("CARGO_MANIFEST_DIR")).join(source), &repo);
        let mut fixture = Self {
            base,
            repo,
            home,
            map_output: String::new(),
        };
        fixture.ok(&["init", "--no-hooks", "--no-map"]);
        fixture.map_output = fixture.ok(&["map", "."]);
        fixture
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .args(args)
            .current_dir(&self.repo)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let result = self.run(args);
        assert!(
            result.status.success(),
            "{args:?}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap()
    }

    fn graph(&self) -> Value {
        serde_json::from_str(
            &fs::read_to_string(self.repo.join(".scout/graph/graph.json")).unwrap(),
        )
        .unwrap()
    }

    fn manifest(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.repo.join(".scout/manifest.json")).unwrap())
            .unwrap()
    }
}

fn fresh(name: &str) -> Fixture {
    Fixture::build(&format!("fixtures/file-scope-defs/{name}"))
}

fn def<'a>(graph: &'a Value, id: &str) -> &'a Value {
    graph["defs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == id)
        .unwrap_or_else(|| panic!("missing {id}"))
}

fn baseline(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("fixtures/file-scope-defs/baseline/{name}.json"));
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn file_scope_defs_ac_1_program_declaration_kinds_and_lines() {
    let graph = fresh("kinds").graph();
    for (id, kind, line) in [
        ("kinds.ts#plain", "function", 1),
        ("kinds.ts#generated", "function", 2),
        ("kinds.ts#PlainClass", "class", 3),
        ("kinds.ts#AbstractClass", "class", 4),
        ("kinds.ts#PlainInterface", "interface", 5),
        ("kinds.ts#PlainType", "type", 6),
        ("kinds.ts#PlainEnum", "enum", 7),
        ("kinds.ts#alpha", "const", 8),
        ("kinds.ts#beta", "const", 8),
        ("kinds.ts#renamed", "const", 8),
        ("kinds.ts#delta", "const", 9),
        ("kinds.ts#epsilon", "const", 10),
        ("kinds.ts#kept", "const", 11),
        ("kinds.ts#dynamic", "const", 12),
        ("kinds.js#plainJs", "function", 1),
        ("kinds.js#renamedJs", "const", 4),
    ] {
        let row = def(&graph, id);
        assert_eq!(row["kind"], kind, "{id}");
        assert_eq!(row["line"], line, "{id}");
        assert!(row["endLine"].as_u64().unwrap() >= line, "{id}");
        assert_eq!(row["local"], true, "{id}");
    }
    for id in ["kinds.ts#imported", "kinds.js#requiredJs"] {
        assert!(!graph["defs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == id));
    }
}

#[test]
fn file_scope_defs_ac_2_direct_assigned_nested_and_excluded_iifes() {
    let graph = fresh("iife").graph();
    for name in [
        "normal",
        "insideCall",
        "arrowed",
        "asyncNormal",
        "asyncArrow",
        "assigned",
        "initialised",
        "nested",
    ] {
        assert_eq!(def(&graph, &format!("forms.js#{name}"))["local"], true);
    }
    for name in ["tsNormal", "tsArrow", "tsInitialised"] {
        assert_eq!(def(&graph, &format!("forms.ts#{name}"))["local"], true);
    }
    assert_eq!(def(&graph, "forms.js#Holder")["local"], true);
    assert_eq!(def(&graph, "forms.js#programFirst")["line"], 1);
    for name in [
        "hiddenInFunction",
        "hiddenInBlock",
        "hiddenInMethod",
        "hiddenInCall",
        "hiddenInUnary",
        "hiddenInArgument",
        "boundInside",
    ] {
        assert!(
            !graph["defs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["name"] == name),
            "{name}"
        );
    }
}

#[test]
fn file_scope_defs_ac_3_same_file_call_jsx_dispatch_and_queries() {
    let fixture = fresh("refs");
    let graph = fixture.graph();
    assert_eq!(def(&graph, "main.tsx#localObject")["local"], true);
    assert!(!graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["to"] == "main.tsx#localObject"));
    assert_eq!(graph["stats"]["ts"]["unresolved_ref_count"], 0);
    for (kind, target) in [
        ("call", "main.tsx#localAction"),
        ("call", "main.tsx#LocalWidget"),
        ("jsx-use", "main.tsx#LocalWidget"),
        ("jsx-use", "main.jsx#LocalBanner"),
        ("dispatch", "main.tsx#localAction"),
    ] {
        assert!(
            graph["edges"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["kind"] == kind
                    && e["to"] == target
                    && e["from_file"] == target.split('#').next().unwrap()),
            "{kind} {target}"
        );
    }
    assert!(fixture
        .ok(&["read", "main.tsx#localAction"])
        .contains("def: main.tsx:1-1"));
    assert!(fixture
        .ok(&["refs", "main.tsx#localAction"])
        .contains("dispatch"));
}

#[test]
fn file_scope_defs_ac_4_imports_never_bind_local_defs() {
    let graph = fresh("cross").graph();
    let old = baseline("cross");
    assert_eq!(
        graph["stats"]["ts"]["unresolved_ref_count"],
        old["tsStats"]["unresolved_ref_count"]
    );
    assert_eq!(
        graph["stats"]["ts"]["external_import_count"],
        old["tsStats"]["external_import_count"]
    );
    for edge in graph["edges"].as_array().unwrap() {
        if edge["from_file"] != "source.ts" {
            assert_ne!(edge["to"], "source.ts#hidden");
        }
        if edge["kind"] == "import" {
            assert!(!edge["bound_defs"]
                .as_array()
                .is_some_and(|ids| ids.iter().any(|id| id == "source.ts#hidden")));
        }
    }
}

#[test]
fn file_scope_defs_ac_5_exported_rows_edges_and_purposes_match_base() {
    let roots = [
        ("conformance-typescript", "fixtures/conformance/typescript"),
        ("ts-query-edges", "fixtures/ts-query-edges"),
        ("ts-resolution", "fixtures/ts-resolution"),
        ("module-extensions", "fixtures/module-extensions"),
        ("ts-grammar", "fixtures/ts-grammar"),
        ("mixed", "fixtures/file-scope-defs/mixed"),
        ("cross", "fixtures/file-scope-defs/cross"),
        ("ties", "fixtures/file-scope-defs/ties"),
    ];
    for (name, path) in roots {
        let fixture = Fixture::build(path);
        let graph = fixture.graph();
        let old = baseline(name);
        let exported: Vec<Value> = graph["defs"].as_array().unwrap().iter().filter(|d| d.get("local").is_none())
            .map(|d| json!({"id": d["id"], "name": d["name"], "namespace": d["namespace"], "kind": d["kind"], "file": d["file"], "line": d["line"], "endLine": d["endLine"]})).collect();
        assert_eq!(
            exported,
            *old["defs"].as_array().unwrap(),
            "{name} exported defs"
        );
        let mut edges = graph["edges"].as_array().unwrap().clone();
        for edge in old["edges"].as_array().unwrap() {
            let i = edges
                .iter()
                .position(|candidate| candidate == edge)
                .unwrap_or_else(|| panic!("{name} missing edge {edge}"));
            edges.remove(i);
        }
        assert!(
            edges
                .iter()
                .all(|e| e["to"].as_str().is_some_and(|id| graph["defs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|d| d["id"] == id && d["local"] == true))),
            "{name} unexpected added edges: {edges:?}"
        );
        assert_eq!(
            graph["stats"]["ts"]["unresolved_ref_count"], old["tsStats"]["unresolved_ref_count"],
            "{name}"
        );
        assert_eq!(
            graph["stats"]["ts"]["external_import_count"], old["tsStats"]["external_import_count"],
            "{name}"
        );
        let manifest = fixture.manifest();
        for (file, entry) in old["manifest"].as_object().unwrap() {
            assert_eq!(
                manifest["entries"][file]["purpose"], entry["purpose"],
                "{name} {file}"
            );
            assert_eq!(
                manifest["entries"][file]["source"], entry["source"],
                "{name} {file}"
            );
        }
    }
}

#[test]
fn file_scope_defs_ac_6_exported_preference_and_local_ambiguity() {
    let fixture = fresh("ties");
    assert!(fixture.ok(&["read", "same"]).starts_with("a.ts#same"));
    assert!(fixture.ok(&["refs", "same"]).starts_with("a.ts#same"));
    assert!(fixture
        .ok(&["tests", "same"])
        .starts_with("tests for a.ts#same"));
    assert!(String::from_utf8(fixture.run(&["impact", "same"]).stdout)
        .unwrap()
        .contains("seed files: a.ts"));
    assert!(fixture
        .ok(&["find", "same"])
        .contains("next: devscout read 'a.ts#same'"));
    assert!(fixture.ok(&["read", "config"]).starts_with("a.ts#Config"));
    assert!(fixture.ok(&["read", "b.ts#same"]).starts_with("b.ts#same"));
    assert!(fixture
        .ok(&["read", "sub/deep.ts#deepLocal"])
        .starts_with("sub/deep.ts#deepLocal"));
    let impact = fixture.run(&["impact", "sub/deep.ts#deepLocal"]);
    assert!(String::from_utf8(impact.stdout)
        .unwrap()
        .contains("seed files: sub/deep.ts"));
    assert!(fixture
        .ok(&["tests", "Save"])
        .starts_with("tests for Demo.Widget"));
    let prior = baseline("ties");
    for (verb, query) in [("read", "config"), ("tests", "Save"), ("impact", "Save")] {
        let result = fixture.run(&[verb, query]);
        let recorded = &prior["queries"][format!("{verb} {query}")];
        assert_eq!(
            result.status.code(),
            recorded["exit"].as_i64().map(|n| n as i32)
        );
        assert_eq!(
            String::from_utf8(result.stdout).unwrap().lines().next(),
            recorded["stdout"].as_str().unwrap().lines().next()
        );
    }
    for (verb, query, ids) in [
        ("read", "many", ["a.ts#many", "b.ts#many", "c.ts#many"]),
        ("read", "alone", ["b.ts#alone", "c.ts#alone", ""]),
    ] {
        let result = fixture.run(&[verb, query]);
        assert_eq!(result.status.code(), Some(1), "{verb} {query}");
        let out = String::from_utf8(result.stdout).unwrap();
        for id in ids.into_iter().filter(|id| !id.is_empty()) {
            assert!(out.contains(id), "{out}");
        }
    }
    let member = fixture.run(&["read", "Save"]);
    assert!(String::from_utf8(member.stdout)
        .unwrap()
        .starts_with("Demo.Widget.Save"));
}

#[test]
fn file_scope_defs_ac_7_find_declaration_rows_and_graph_names() {
    let fixture = fresh("mixed");
    let graph = fixture.graph();
    for d in graph["defs"].as_array().unwrap() {
        let matching: Vec<&Value> = graph["names"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["name"] == d["name"] && n["file"] == d["file"] && n["line"] == d["line"])
            .collect();
        assert_eq!(matching.len(), 1, "{d}");
        assert!(matching[0].get("owner").is_none());
    }
    let local = fixture.ok(&["find", "localFn"]);
    assert!(local.contains("main.js:2"));
    let public = fixture.ok(&["find", "publicFn"]);
    let declaration = public.find("main.js:1  export function publicFn").unwrap();
    let manifest = public.find("main.js:1:").unwrap();
    assert!(declaration < manifest, "{public}");
}

#[test]
fn file_scope_defs_ac_8_prior_cache_generation_reextracts() {
    let fixture = fresh("mixed");
    let dir = fixture.repo.join(".scout/graph");
    fs::rename(
        dir.join("fragments-v37.json"),
        dir.join("fragments-v33.json"),
    )
    .unwrap();
    fs::rename(
        dir.join("fragments-index-v37.json"),
        dir.join("fragments-index-v33.json"),
    )
    .unwrap();
    fixture.ok(&["map", "."]);
    assert!(dir.join("fragments-v37.json").exists());
    assert!(dir.join("fragments-index-v37.json").exists());
    assert!(!dir.join("fragments-v33.json").exists());
    assert!(!dir.join("fragments-index-v33.json").exists());
    assert_eq!(def(&fixture.graph(), "main.js#localFn")["local"], true);
}

#[test]
fn file_scope_defs_ac_8_invalid_local_cache_value_reextracts() {
    let fixture = fresh("mixed");
    let cache = fixture.repo.join(".scout/graph/fragments-v37.json");
    let text = fs::read_to_string(&cache).unwrap();
    assert!(text.contains("\"local\":true"));
    fs::write(
        &cache,
        text.replacen("\"local\":true", "\"local\":\"yes\"", 1),
    )
    .unwrap();
    fixture.ok(&["map", "."]);
    assert_eq!(def(&fixture.graph(), "main.js#localFn")["local"], true);
    assert!(fs::read_to_string(cache)
        .unwrap()
        .contains("\"local\":true"));
}

#[test]
fn file_scope_defs_ac_9_userscript_wrappers_map_to_local_defs() {
    let fixture = fresh("userscript");
    assert!(
        fixture.map_output.contains("3 defs"),
        "{}",
        fixture.map_output
    );
    let graph = fixture.graph();
    assert_eq!(def(&graph, "one.js#issueToken")["local"], true);
    assert!(graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["to"] == "one.js#issueToken" && e["kind"] == "call"));
    assert!(fixture
        .ok(&["read", "one.js#issueToken"])
        .contains("def: one.js:4-4"));
}

#[test]
fn file_scope_defs_ac_10_public_docs_disclose_scope_and_resolution() {
    for text in [
        include_str!("../README.md"),
        include_str!("../docs/explanation/resolution.md"),
        include_str!("../docs/limitations.md"),
        include_str!("../CHANGELOG.md"),
    ] {
        let text = text.to_lowercase();
        assert!(text.contains("iife"));
        assert!(text.contains("file-scope") || text.contains("file scope"));
    }
}
