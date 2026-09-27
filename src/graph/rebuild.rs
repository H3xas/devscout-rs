use std::collections::HashMap;
use std::fs;
use std::io;
use std::io::Read;
use std::path::Path;

use crate::extract;

use super::artifact::{write_graph, Graph, GRAPH_SCHEMA_VERSION};
use super::cache::{
    read_fragments_cache, write_fragments_cache, write_fragments_index, FragmentCacheEntry,
};
use super::fragment_types::{AnyFragment, Fragment};
use super::ordered::OrderedMap;
use super::paths::{
    atomic_write_json, graph_json_path, project_units_path, remove_superseded_caches,
    rust_crates_path,
};

// ---------------------------------------------------------------------------
// `devscout map`'s cache-then-resolve-then-write cycle.
// ---------------------------------------------------------------------------

/// One file that contributes a graph fragment: C# (defs, refs, member names),
/// markup (`x:Class`/`x:Name`, `.resw` keys -- names only), TS/JS
/// (imports, exported declarations, call/JSX/dispatch references) or Rust
/// (items with spans).
#[derive(Debug, Clone)]
pub struct GraphFile {
    /// The rel value.
    pub rel: String,
    /// The mtime value.
    pub mtime: i64,
}

#[derive(Debug)]
/// Represents `RebuildOutcome`.
pub enum RebuildOutcome {
    /// Represents `NotRebuilt`.
    NotRebuilt,
    /// Represents `Rebuilt`.
    Rebuilt(Graph),
}

/// Whether the graph.json already on disk was written at the CURRENT schema
/// version. Reads the first 64 bytes and compares the literal
/// `{"schema_version":N,` prefix rather than deserializing: `schema_version`
/// is the first key `Graph` serializes, the artifact can be tens of
/// megabytes, and this runs on the fast path whose whole point is not opening
/// it. Anything else -- an older version, an unreadable or truncated file --
/// answers false and costs a rebuild, which is the safe direction.
pub(crate) fn graph_schema_is_current(root: &Path) -> bool {
    let Ok(mut file) = fs::File::open(graph_json_path(root)) else {
        return false;
    };
    let mut head = [0u8; 64];
    let mut filled = 0;
    while filled < head.len() {
        match file.read(&mut head[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => return false,
        }
    }
    head[..filled].starts_with(format!("{{\"schema_version\":{GRAPH_SCHEMA_VERSION},").as_bytes())
}

// Mirrors the model's units into the staleness sidecar. No model means the
// repo declares no `.csproj`, and then the file must NOT exist: an empty
// `[]` left behind would be indistinguishable from "no model" on the read
// side, and the sidecar's whole job is telling those two apart.
fn write_project_units(
    root: &Path,
    model: Option<&crate::project::ProjectModel>,
) -> io::Result<()> {
    match model {
        Some(m) => atomic_write_json(&project_units_path(root), &crate::project::graph_units(m)),
        None => {
            let path = project_units_path(root);
            match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            }
        }
    }
}

// Mirrors the Cargo package model into its staleness sidecar, the same
// present-or-absent rule as `write_project_units`.
fn write_rust_crates(
    root: &Path,
    crates: Option<&crate::rsgraph::crates::RustCrates>,
) -> io::Result<()> {
    match crates {
        Some(c) => atomic_write_json(&rust_crates_path(root), c),
        None => match fs::remove_file(rust_crates_path(root)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        },
    }
}

/// `fresh_fragments`: fragments this run's extractor produced for files that
/// needed reparsing (the same set that got a fresh purpose signature in the
/// real `devscout map` flow -- `mapcmd::map_repo` assembles it). `changed`: from the
/// caller's own `indexIsStale`-equivalent check against `csFiles`, passed in
/// rather than recomputed so the unchanged path never opens any graph file.
/// `model`: the repo's discovered `.csproj` projects, or `None` when it
/// declares none -- serialized into `graph.units` and mirrored into the
/// `project_units_path` staleness sidecar. The caller is expected to have
/// already folded `project::sidecar_differs` into `changed`; this function
/// only writes the sidecar, it never decides on it.
pub fn rebuild_graph(
    root: &Path,
    graph_files: &[GraphFile],
    fresh_fragments: &HashMap<String, AnyFragment>,
    changed: bool,
    model: Option<&crate::project::ProjectModel>,
    semantic_layer: Option<&crate::semantic::SemanticLayer>,
) -> io::Result<RebuildOutcome> {
    rebuild_graph_with_crates(
        root,
        graph_files,
        fresh_fragments,
        changed,
        model,
        semantic_layer,
        None,
    )
}

/// `rebuild_graph` with the Cargo package model `map` discovered: Rust
/// paths resolve against its crate roots, and it is mirrored into the
/// `rust_crates_path` sidecar. `None` means no package is in scope.
pub fn rebuild_graph_with_crates(
    root: &Path,
    graph_files: &[GraphFile],
    fresh_fragments: &HashMap<String, AnyFragment>,
    changed: bool,
    model: Option<&crate::project::ProjectModel>,
    semantic_layer: Option<&crate::semantic::SemanticLayer>,
    crates: Option<&crate::rsgraph::crates::RustCrates>,
) -> io::Result<RebuildOutcome> {
    if !changed && graph_json_path(root).exists() && graph_schema_is_current(root) {
        return Ok(RebuildOutcome::NotRebuilt);
    }

    let cache = read_fragments_cache(root);
    // Split at the door: the shapes share no field beyond `defs`, and letting
    // one resolver see another's names would resolve a C# type reference onto
    // a same-named TypeScript const. Rust fragments reach no resolver at all;
    // their items are projected after it. Built as separate vecs here rather
    // than one mixed vec split later so no fragment is cloned a third time on
    // the rebuild path.
    let mut merged_cs: Vec<(String, Fragment)> = Vec::new();
    let mut merged_ts: Vec<(String, extract::TsFragment)> = Vec::new();
    let mut merged_rs: Vec<(String, extract::RsFragment)> = Vec::new();
    let mut new_cache: OrderedMap<FragmentCacheEntry> = OrderedMap::new();
    for f in graph_files {
        let fragment = match cache.get(&f.rel) {
            Some(entry) if entry.mtime == f.mtime => Some(entry.fragment.clone()),
            _ => fresh_fragments.get(&f.rel).cloned(),
        };
        let Some(fragment) = fragment else { continue };
        match &fragment {
            AnyFragment::Cs(c) => merged_cs.push((f.rel.clone(), c.clone())),
            AnyFragment::Ts(t) => merged_ts.push((f.rel.clone(), t.clone())),
            AnyFragment::Rs(r) => merged_rs.push((f.rel.clone(), r.clone())),
        }
        new_cache.insert(
            f.rel.clone(),
            FragmentCacheEntry {
                mtime: f.mtime,
                fragment,
            },
        );
    }

    let mut graph = crate::resolve::resolve_graph_with_model(
        root,
        &merged_cs,
        &merged_ts,
        model,
        semantic_layer,
    );
    crate::rsgraph::append_rust(&mut graph, &merged_rs);
    crate::rsgraph::append_rust_edges(&mut graph, &merged_rs, crates);
    write_graph(root, &graph)?;
    write_project_units(root, model)?;
    write_rust_crates(root, crates)?;
    write_fragments_cache(root, &new_cache)?;
    write_fragments_index(root, &new_cache)?;
    remove_superseded_caches(root);
    Ok(RebuildOutcome::Rebuilt(graph))
}
