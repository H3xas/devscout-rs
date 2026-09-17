// What a candidate artifact is admitted against: the engine revision,
// contract version and dependency fingerprint pinned to the shipped engine
// build (compiled-in constants, following the crate's existing exact-pin
// convention in Cargo.toml), the profile the caller requested, and the
// source-snapshot identity read from this checkout's own git HEAD via the
// crate's one existing `git` shell-out (`manifest::git_head`) -- no new
// shell-out site is added anywhere in this crate.

use std::path::Path;

use crate::manifest;

use super::artifact::{Profile, COMPILER_FACTS_CONTRACT_VERSION};

/// The engine-protocol revision this admission path expects. Distinct from
/// [`super::artifact::COMPILER_FACTS_CONTRACT_VERSION`]: this names the
/// engine's own analysis behaviour, the contract names the wire shape.
/// Bump together with a corresponding change on the engine side.
pub const EXPECTED_ENGINE_REVISION: &str = "1";

/// The sha256 digest of `tools/scout-semantic/packages.lock.json` as of the
/// engine build this admission path expects. Recompute
/// (`shasum -a 256 tools/scout-semantic/packages.lock.json`) and update this
/// constant whenever that lock file changes.
pub const EXPECTED_DEPENDENCY_FINGERPRINT: &str =
    "f0e2aa25d0071aab4aa9de47f3a7629b783a5f17bf625b565f073b48e69d0c83";

/// The compilation-context envelope version this admission path recognises.
/// A candidate embedding any other version is refused without this module
/// ever parsing the envelope's internal shape.
pub const EXPECTED_CONTEXT_SCHEMA_VERSION: u64 = 1;

/// What one admission call checks a candidate against.
#[derive(Debug, Clone)]
pub struct AdmissionExpectations {
    /// The wire-contract version the candidate must declare.
    pub contract_version: u64,
    /// The engine revision the candidate must report.
    pub engine_revision: &'static str,
    /// The dependency fingerprint the candidate must report.
    pub dependency_fingerprint: &'static str,
    /// The compilation-context envelope version the candidate must embed.
    pub context_schema_version: u64,
    /// The profile the caller requested of the engine.
    pub profile: Profile,
    /// This checkout's own HEAD commit, or `None` outside a git repository.
    /// `None` skips the source-snapshot check rather than refusing every
    /// candidate on a checkout admission cannot identify.
    pub head_sha: Option<String>,
}

/// Builds the expectations a `compiler-facts run`/`import` admits against,
/// for the checkout at `root` and the profile the caller requested.
pub fn expectations_for(root: &Path, profile: Profile) -> AdmissionExpectations {
    AdmissionExpectations {
        contract_version: COMPILER_FACTS_CONTRACT_VERSION,
        engine_revision: EXPECTED_ENGINE_REVISION,
        dependency_fingerprint: EXPECTED_DEPENDENCY_FINGERPRINT,
        context_schema_version: EXPECTED_CONTEXT_SCHEMA_VERSION,
        profile,
        head_sha: manifest::git_head(root),
    }
}
