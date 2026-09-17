// The compiler-facts artifact: a versioned, atomically published record of
// what an optional one-shot Roslyn engine run (or a build/CI-produced
// import of one) discovered, admitted through exactly one Rust path
// regardless of which producer supplied the candidate bytes. Lives beside
// `graph.json` under the same graph directory, but is its own file with its
// own schema -- admitting one never touches `graph.json` or bumps
// `GRAPH_SCHEMA_VERSION`, the same boundary `graph::imports` already draws
// for the cross-repo edge artifact.
//
// `map` and every query never spawn the engine and never require the
// network: this module is reached only from the `compiler-facts` CLI verb
// group. An absent artifact is the ordinary "no engine ever ran here"
// state, read back through `read_compiler_facts`'s fail-open `None`.

mod admit;
mod artifact;
mod expectations;
mod paths;
mod reasons;
mod run;

pub use admit::{admit, AdmittedFacts};
pub use artifact::{
    Coverage, IncompleteUnit, Profile, COMPILER_FACTS_ARTIFACT_SCHEMA_VERSION,
    COMPILER_FACTS_CONTRACT_VERSION, COMPILER_FACTS_FORMAT,
};
pub use expectations::{
    expectations_for, AdmissionExpectations, EXPECTED_CONTEXT_SCHEMA_VERSION,
    EXPECTED_DEPENDENCY_FINGERPRINT, EXPECTED_ENGINE_REVISION,
};
pub use paths::compiler_facts_json_path;
pub use reasons::RefusalReason;
pub use run::{run_engine, DEFAULT_OUTPUT_CAP, DEFAULT_TIMEOUT};

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::paths::atomic_write_bytes;

/// The engine locator environment variable: the path to the engine
/// executable `compiler-facts run` spawns. Read only from this module, with
/// no default and no download -- a missing engine exits non-zero with one
/// line and leaves any already-admitted artifact untouched. Named
/// distinctly from the unrelated, unshipped `SCOUT_SEMANTIC_TOOL` locator a
/// different design note proposes for a different, still-unbuilt cache, so
/// the two invocation paths can never be confused once both exist.
pub const SCOUT_COMPILER_ENGINE: &str = "SCOUT_COMPILER_ENGINE";

/// Locates the engine executable from `SCOUT_COMPILER_ENGINE`, or `None`
/// when the variable is unset or empty.
pub fn locate_engine() -> Option<PathBuf> {
    let raw = std::env::var(SCOUT_COMPILER_ENGINE).ok()?;
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

/// Why `admit_and_publish` did not produce an admitted, published artifact:
/// either the candidate itself was refused, or admission's own atomic write
/// failed. The two are kept apart because only the first is a
/// `RefusalReason` a caller might match on by token; the second is an
/// ordinary filesystem failure.
#[derive(Debug)]
pub enum PublishError {
    /// The candidate was refused; nothing was written.
    Refused(RefusalReason),
    /// The candidate was admitted but the atomic write itself failed.
    /// Whatever artifact already existed is untouched -- the write only
    /// ever replaces the target once its temp file is fully written.
    Io(io::Error),
}

impl From<RefusalReason> for PublishError {
    fn from(reason: RefusalReason) -> Self {
        PublishError::Refused(reason)
    }
}

/// Admits `candidate_bytes` and, on success, publishes it atomically as
/// this checkout's compiler-facts artifact -- the one place both `run` and
/// `import` converge, the concrete mechanism behind "one admission path for
/// both producers." On any refusal, no write occurs and whatever artifact
/// already existed is untouched.
pub fn admit_and_publish(
    root: &Path,
    candidate_bytes: &[u8],
    expected: &AdmissionExpectations,
) -> Result<AdmittedFacts, PublishError> {
    let facts = admit::admit(candidate_bytes, expected)?;
    atomic_write_bytes(&paths::compiler_facts_json_path(root), &facts.bytes)
        .map_err(PublishError::Io)?;
    paths::remove_superseded_compiler_facts(root);
    Ok(facts)
}

/// Reads this checkout's admitted compiler-facts artifact, or `None` when
/// absent or unreadable -- the same fail-open convention
/// `graph::read_graph`/`graph::read_imported_edges` already follow, since
/// this artifact is auxiliary and its absence is the ordinary "no engine
/// ever ran here" state, not an error.
pub fn read_compiler_facts(root: &Path) -> Option<AdmittedFacts> {
    let bytes = fs::read(paths::compiler_facts_json_path(root)).ok()?;
    let header = artifact::parse_header(&bytes).ok()?;
    Some(AdmittedFacts { header, bytes })
}
