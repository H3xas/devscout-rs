//! Every artifact file name and directory segment devscout writes.
//!
//! Paths are built from these values, so a name has one definition in the
//! source and the storage reference is generated from the same constants:
//! renaming a file, or advancing the fragment generation, is a change here
//! plus a regeneration rather than a search through the path builders and the
//! messages that name an artifact.
//!
//! Names of files that are superseded rather than written live with the code
//! that retires them (`graph::paths::SUPERSEDED_CACHE_FILES`), not here.

/// The artifact directory inside a repository's Git common directory, shared
/// by every linked worktree.
pub const SCOUT_DIR: &str = "scout";

/// The graph artifacts' subdirectory of [`SCOUT_DIR`].
pub const GRAPH_DIR: &str = "graph";

/// The telemetry log's subdirectory of [`SCOUT_DIR`], and of [`FALLBACK_DIR`].
pub const LOG_DIR: &str = "log";

/// The agent-settings directory under `$HOME`, holding the hook settings file
/// and the registry.
pub const AGENT_DIR: &str = ".claude";

/// The artifact directory under the repository root, used when there is no Git
/// common directory to hang artifacts off.
pub const FALLBACK_DIR: &str = ".scout";

/// The directory entry whose presence turns hook debug logging on, like
/// `SCOUT_DEBUG=1`.
pub const DEBUG_MARKER: &str = "debug";

/// File to purpose and symbol index.
pub const MANIFEST_FILE: &str = "manifest.json";

/// The registry of roots devscout knows about.
pub const REGISTRY_FILE: &str = "repos.json";

/// Hook freshness cache.
pub const CACHE_DB_FILE: &str = "cache.db";

/// The cross-repo content-addressed dedup database.
pub const CONTENT_DB_FILE: &str = "content.db";

/// The freshness record: owner worktree, generation, indexed HEAD, digests.
pub const INDEX_STATE_FILE: &str = "index-state.json";

/// Per-file content digest for every indexed file of the current generation.
pub const INDEX_DIGESTS_FILE: &str = "index-digests.json";

/// The lock a `map` holds while it publishes.
pub const MAP_LOCK_FILE: &str = "map.lock";

/// Definitions, edges and project units.
pub const GRAPH_FILE: &str = "graph.json";

/// The current generation's fragment cache.
///
/// The generation in the name is the fragment schema version, bumped whenever
/// the extractor starts recording something cached fragments lack; the rename
/// IS the invalidation mechanism, so the number a reader reports can only be
/// the one this name carries.
pub const FRAGMENT_CACHE_FILE: &str = "fragments-v42.json";

/// The fragment cache's own index of what it holds, at the same generation.
pub const FRAGMENT_INDEX_FILE: &str = "fragments-index-v42.json";

/// csproj staleness sidecar, written only with a project model.
pub const PROJECT_UNITS_FILE: &str = "project-units.json";

/// Cargo crate-model staleness sidecar, written only with a Cargo package in
/// scope.
pub const RUST_CRATES_FILE: &str = "rust-crates.json";

/// Transport-input staleness sidecar, written only with a transport site.
///
/// It holds the namespaces each project's SDK and `Using` items import, and
/// the digest of each settings file a transport binding reads.
pub const TRANSPORT_INPUTS_FILE: &str = "transport-inputs.json";

/// The imported cross-repo edge set `import-edges` writes.
pub const IMPORTED_EDGES_FILE: &str = "imported-edges.json";

/// Query-verb telemetry, one JSON line per answered invocation.
pub const QUERIES_LOG_FILE: &str = "queries.jsonl";

/// The hook debug log.
pub const DEBUG_LOG_FILE: &str = "debug.log";
