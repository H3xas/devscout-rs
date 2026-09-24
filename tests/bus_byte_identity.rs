//! Byte identity for `map` on a repository with no message-bus site at all.
//! A later lane adds an optional per-edge hop-count field that must stay
//! ABSENT (not present-and-zero) when no bus hop exists, and must serialize
//! after every existing key so nothing already emitted shifts position. The
//! two fixtures here contain no bus call anywhere, so today's `map` output
//! is the baseline that field is required to leave untouched: pinning it
//! byte-for-byte means an added-but-empty key, or a key inserted anywhere
//! but last, changes the hash and fails loudly.
//!
//! Harness follows `tests/import_edges_byte_identity.rs`: a per-process temp
//! root, `SCOUT_REGISTRY` isolation, `init --no-hooks`, `map`, then a read
//! of `.scout/graph/graph.json`. These two fixtures are nested trees rather
//! than the flat file list that harness copies, so the copy step walks the
//! tree instead, the same way `tests/semantic_audit.rs` and
//! `tests/semantic_audit_direction.rs` already stage these exact fixtures.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    registry: PathBuf,
}

impl Fixture {
    fn new(fixture_name: &str, trees: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-bus-byte-identity-{fixture_name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(fixture_name);
        for tree in trees {
            copy_tree_skip_build_output(&base.join(tree), &root.join(tree));
        }
        let registry = std::env::temp_dir().join(format!(
            "devscout-bus-byte-identity-registry-{fixture_name}-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let fixture = Self { root, registry };
        let init = fixture.run(&["init", "--no-hooks"]);
        assert!(init.status.success(), "init failed: {init:?}");
        let mut map_args = vec!["map"];
        map_args.extend_from_slice(trees);
        let map = fixture.run(&map_args);
        assert!(map.status.success(), "map failed: {map:?}");
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

    fn graph_json_bytes(&self) -> Vec<u8> {
        fs::read(self.root.join(".scout/graph/graph.json")).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_file(&self.registry);
    }
}

/// Recursive copy skipping any directory named `bin` or `obj`, the same
/// exclusion `tests/semantic_audit.rs` and `tests/semantic_audit_direction.rs`
/// already apply when staging these fixtures for indexing.
fn copy_tree_skip_build_output(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let file_type = entry.file_type().unwrap();
        if file_type.is_dir() {
            if name == "bin" || name == "obj" {
                continue;
            }
            copy_tree_skip_build_output(&entry.path(), &dst.join(&name));
        } else if file_type.is_file() {
            fs::copy(entry.path(), dst.join(&name)).unwrap();
        }
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

const SCHEMA_PREFIX: &str = r#"{"schema_version":3,"#;

// Reference bytes re-derived from the release/0.7.0 base binary (commit
// 771d5b6d49e07968362a447416cc2338ef5fc9e2), the last point before any
// bus-feature edit landed. `map` over a bus-free repository must keep
// producing them unchanged until a bus hop actually appears in the source.
const SEMANTIC_LEN: usize = 39966;
const SEMANTIC_SHA256: &str = "c0e5ef806be97b02f37fb6e7db0e64622d9de53a9f86290d518f6cce2f3b3102";

const DIRECTION_LEN: usize = 29874;
const DIRECTION_SHA256: &str = "2dd18553dc4ba2b74d5904bfe01193db555e54df60f09d67a4668e6a0d47d15a";

#[test]
fn graph_json_stays_byte_identical_on_csharp_semantic_which_has_no_bus_site() {
    let fx = Fixture::new("csharp-semantic", &["src", "tests"]);
    let bytes = fx.graph_json_bytes();
    assert_eq!(
        bytes.len(),
        SEMANTIC_LEN,
        "graph.json length moved on a repository with no bus hop"
    );
    assert_eq!(
        hex_sha256(&bytes),
        SEMANTIC_SHA256,
        "graph.json bytes moved on a repository with no bus hop"
    );
    assert!(
        bytes.starts_with(SCHEMA_PREFIX.as_bytes()),
        "schema_version must stay 3"
    );
}

#[test]
fn graph_json_stays_byte_identical_on_csharp_direction_which_has_no_bus_site() {
    let fx = Fixture::new("csharp-direction", &["src"]);
    let bytes = fx.graph_json_bytes();
    assert_eq!(
        bytes.len(),
        DIRECTION_LEN,
        "graph.json length moved on a repository with no bus hop"
    );
    assert_eq!(
        hex_sha256(&bytes),
        DIRECTION_SHA256,
        "graph.json bytes moved on a repository with no bus hop"
    );
    assert!(
        bytes.starts_with(SCHEMA_PREFIX.as_bytes()),
        "schema_version must stay 3"
    );
}
