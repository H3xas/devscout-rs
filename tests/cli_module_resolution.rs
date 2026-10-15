//! End-to-end module resolution cases scored against TypeScript targets.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}

struct Case {
    root: PathBuf,
    repo: PathBuf,
    home: PathBuf,
}

impl Case {
    fn new(name: &str) -> Self {
        let root = env::temp_dir().join(format!(
            "devscout-module-resolution-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let repo = root.join("repo");
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/module-resolution")
            .join(name);
        copy_tree(&fixture, &repo);
        let case = Self { root, repo, home };
        case.run(&["init", "--no-hooks", "--no-map"]);
        case.run(&["map", "."]);
        case
    }

    fn run(&self, args: &[&str]) {
        let output = Command::new(env!("CARGO_BIN_EXE_devscout"))
            .args(args)
            .current_dir(&self.repo)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}: {args:?}: {}",
            self.repo.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn graph(&self) -> Value {
        serde_json::from_str(
            &fs::read_to_string(self.repo.join(".scout/graph/graph.json")).unwrap(),
        )
        .unwrap()
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn module_resolution_ac_8_synthetic_targets_and_reasons_match_truth() {
    let expected = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/module-resolution/expected.tsv"),
    )
    .unwrap();
    let mut current_name = String::new();
    let mut current = None;
    for row in expected.lines().skip(1) {
        let parts: Vec<&str> = row.split('\t').collect();
        assert_eq!(parts.len(), 5, "{row}");
        let [name, file, line, spec, target] = [parts[0], parts[1], parts[2], parts[3], parts[4]];
        if name != current_name {
            current = Some(Case::new(name));
            current_name = name.to_string();
        }
        let graph = current.as_ref().unwrap().graph();
        let line: u64 = line.parse().unwrap();
        let imports: Vec<&Value> = graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|edge| {
                edge["kind"] == "import"
                    && edge["from_file"] == file
                    && edge["from_line"] == line
                    && edge["target"] == spec
                    && edge["via"].is_null()
            })
            .collect();
        if let Some(reason) = target.strip_prefix("unresolved:") {
            assert!(imports.is_empty(), "{row}: unexpected edges: {imports:?}");
            let unresolved = graph["unresolved_imports"]
                .as_array()
                .unwrap_or_else(|| panic!("{row}: no unresolved_imports"));
            assert!(
                unresolved.iter().any(|entry| {
                    entry["from_file"] == file
                        && entry["from_line"] == line
                        && entry["target"] == spec
                        && entry["reason"] == reason
                }),
                "{row}: {unresolved:?}"
            );
        } else {
            assert!(
                imports.iter().any(|edge| edge["to_file"] == target),
                "{row}: {imports:?}"
            );
        }
    }
}

#[test]
fn module_resolution_ac_5_module_forms_bind_calls_and_count_nonliteral_imports() {
    for (name, target, expected_calls) in [
        ("forms-esm", "src/target.ts", 4),
        ("forms-cjs", "src/target.ts", 3),
    ] {
        let graph = Case::new(name).graph();
        let calls = graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|edge| {
                edge["kind"] == "call"
                    && edge["from_file"] == "src/main.ts"
                    && edge["to_file"] == target
            })
            .count();
        assert_eq!(calls, expected_calls, "{name}: {}", graph["edges"]);
        assert_eq!(
            graph["stats"]["ts"]["non_literal_dynamic_import_count"], 1,
            "{name}"
        );
    }
}

#[test]
fn module_resolution_ac_7_no_mode_does_not_read_bad_configuration() {
    let graph = Case::new("no-mode").graph();
    assert!(graph.get("unresolved_imports").is_none());
    assert!(graph["stats"]["ts"].get("refused_config").is_none());
    assert!(graph["edges"].as_array().unwrap().iter().any(|edge| {
        edge["kind"] == "import"
            && edge["from_file"] == "src/main.ts"
            && edge["to_file"] == "src/target.ts"
    }));
}

#[test]
fn module_resolution_ac_1_classic_is_reported_and_uses_legacy_resolution() {
    let graph = Case::new("classic").graph();
    assert_eq!(
        graph["stats"]["ts"]["unsupported_mode_scopes"][0]["mode"],
        "classic"
    );
    assert!(graph["edges"].as_array().unwrap().iter().any(|edge| {
        edge["kind"] == "import"
            && edge["from_file"] == "src/main.ts"
            && edge["to_file"] == "src/target.ts"
    }));
}

#[test]
fn module_resolution_ac_6_refused_inputs_are_disclosed() {
    for (name, config_file, field) in [
        ("refused-workspace", "pnpm-workspace.yaml", "packages"),
        ("refused-manifest", "package.json", "document"),
    ] {
        let graph = Case::new(name).graph();
        assert_eq!(
            graph["unresolved_imports"][0]["reason"], "configuration-refused",
            "{name}"
        );
        assert!(
            graph["stats"]["ts"]["refused_config"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["file"] == config_file && row["field"] == field),
            "{name}"
        );
    }
}
