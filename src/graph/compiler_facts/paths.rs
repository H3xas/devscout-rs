// Path resolution for the compiler-facts artifact. Reuses the already-public
// `graph::graph_json_path` instead of reaching for the private `graph_dir`,
// so this file needs no `super::paths` access at all despite living beside
// `graph.json` in the same directory.

use std::fs;
use std::path::{Path, PathBuf};

use crate::graph::graph_json_path;

use super::artifact::COMPILER_FACTS_CONTRACT_VERSION;

/// Every generation below the current one. Empty at contract version 1;
/// filled in the same way `graph::paths::SUPERSEDED_CACHE_FILES` already
/// is, the moment a second contract version ships.
pub const SUPERSEDED_COMPILER_FACTS_FILES: &[&str] = &[];

/// The compiler-facts artifact's file name: the contract version is part of
/// it, so a future contract bump writes a new file rather than overwriting
/// this one -- a v2-only reader is inert to a stale v1 file by construction.
/// The one place the name is spelled.
#[must_use]
pub fn compiler_facts_file_name() -> String {
    format!("compiler-facts-v{COMPILER_FACTS_CONTRACT_VERSION}.json")
}

/// Path to the compiler-facts artifact for `root`, beside `graph.json`.
pub fn compiler_facts_json_path(root: &Path) -> PathBuf {
    graph_json_path(root).with_file_name(compiler_facts_file_name())
}

/// Removes every superseded compiler-facts generation for `root`. Called
/// from the publish step; a no-op today since
/// [`SUPERSEDED_COMPILER_FACTS_FILES`] is empty at contract version 1.
pub fn remove_superseded_compiler_facts(root: &Path) {
    for name in SUPERSEDED_COMPILER_FACTS_FILES {
        let _ = fs::remove_file(graph_json_path(root).with_file_name(name));
    }
}
