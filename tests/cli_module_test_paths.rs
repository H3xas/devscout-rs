//! End-to-end path classification for TypeScript module and JSX test files.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = env::temp_dir().join(format!(
        "scout-module-test-paths-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    fs::canonicalize(&dir).expect("canonicalize temp dir")
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/module-test-paths")
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create dest dir");
    for entry in fs::read_dir(src).expect("read source dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name();
        let file_type = entry.file_type().expect("entry file type");
        if file_type.is_dir() {
            copy_tree(&entry.path(), &dst.join(&name));
        } else if file_type.is_file() && name != "README.md" {
            fs::copy(entry.path(), dst.join(&name)).expect("copy fixture file");
        }
    }
}

struct Fixture {
    base: PathBuf,
    repo: PathBuf,
    home: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

impl Fixture {
    fn build() -> Fixture {
        let base = temp_dir();
        let repo = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&home).expect("create home dir");
        copy_tree(&fixture_root(), &repo);
        let fx = Fixture { base, repo, home };
        fx.expect_ok(&["init", "--no-hooks", "--no-map"]);
        fx.expect_ok(&["map", "."]);
        fx
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .args(args)
            .current_dir(&self.repo)
            .env("HOME", &self.home)
            .env("SCOUT_REGISTRY", self.home.join("repos.json"))
            .env("SCOUT_CONTENT_DB", self.home.join("content.db"))
            .output()
            .expect("devscout must run")
    }

    fn expect_ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "devscout {args:?} failed: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("stdout is utf-8")
    }

    fn json(&self, args: &[&str]) -> Value {
        serde_json::from_str(self.expect_ok(args).trim()).expect("valid JSON")
    }
}

#[test]
fn module_test_paths_ac_3_counts_paths_without_losing_near_miss_refs() {
    let fx = Fixture::build();
    let expected: BTreeSet<&str> = [
        "__tests__/root.jsx",
        "src/__tests__/deep/check.cts",
        "src/__tests__/deep/check.mts",
        "src/amount.spec.cts",
        "src/amount.test.jsx",
        "src/amount.test.mts",
    ]
    .into_iter()
    .collect();
    let tests = fx.json(&["tests", "total", "--json"]);
    let rows = tests["rows"].as_array().expect("test rows");
    let paths: BTreeSet<&str> = rows
        .iter()
        .map(|row| {
            assert_eq!(row["via"], "path");
            assert_eq!(row["why"], "test-path");
            assert!(!row["lines"].as_array().expect("lines").is_empty());
            row["file"].as_str().expect("file")
        })
        .collect();
    assert_eq!(paths, expected);
    assert_eq!(rows.len(), 6);
    assert_eq!(tests["testFileCount"], 6);

    let impact = fx.json(&["impact", "src/lib/money.mjs", "--hops", "1", "--json"]);
    assert_eq!(impact["testsAffected"], 6);

    let refs = fx.json(&["refs", "total", "--all", "--json"]);
    let referrers: BTreeSet<&str> = refs["inbound"]["call"]["rows"]
        .as_array()
        .expect("call rows")
        .iter()
        .map(|row| row["file"].as_str().expect("file"))
        .collect();
    for control in [
        "src/upper.TEST.mts",
        "src/__tests__x/check.cts",
        "tests/check.jsx",
    ] {
        assert!(
            referrers.contains(control),
            "{control} missing from refs: {referrers:?}"
        );
        assert!(!paths.contains(control), "{control} became a test path");
    }
}
