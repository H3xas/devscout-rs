use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::crates::{discover, RustCrates};
use crate::extract::{extract_rs_file, RsFragment};
use crate::graph::{Edge, Graph};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A scratch tree with the given files, outside any git work tree.
fn scratch(files: &[(&str, &str)]) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("scout-rsgraph-edges-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The packages `discover` finds over a scratch tree of `files`, with every
/// `.rs` file among them walked.
fn crates_of(files: &[(&str, &str)]) -> Option<RustCrates> {
    let dir = scratch(files);
    let rs: Vec<String> = files
        .iter()
        .filter(|(rel, _)| rel.ends_with(".rs"))
        .map(|(rel, _)| rel.to_string())
        .collect();
    let out = discover(&dir, &[".".to_string()], &rs).unwrap();
    let _ = fs::remove_dir_all(&dir);
    out
}

/// The graph `files` project into: every `.rs` file extracted, the Cargo
/// model discovered, and the Rust defs, names and edges appended to an
/// empty graph.
fn graph_for(files: &[(&str, &str)]) -> Graph {
    let crates = crates_of(files);
    let fragments: Vec<(String, RsFragment)> = files
        .iter()
        .filter(|(rel, _)| rel.ends_with(".rs"))
        .map(|(rel, body)| (rel.to_string(), extract_rs_file(body).fragment))
        .collect();
    let root = std::env::temp_dir().join("scout-rsgraph-edges-no-root");
    let mut graph = crate::resolve::resolve_graph_with_model(&root, &[], &[], None, None);
    super::append_rust(&mut graph, &fragments);
    super::append_rust_edges(&mut graph, &fragments, crates.as_ref());
    graph
}

/// Every edge as `kind from_file:from_line -> to`, ambiguous edges as
/// `ambiguous(origin) from_file:from_line raw [candidates]`.
fn edges_for(files: &[(&str, &str)]) -> Vec<String> {
    graph_for(files).edges.iter().map(brief).collect()
}

fn brief(edge: &Edge) -> String {
    match edge {
        Edge::UsesMember {
            from_file,
            from_line,
            to,
            ..
        } => {
            format!("uses-member {from_file}:{from_line} -> {to}")
        }
        Edge::UsesType {
            from_file,
            from_line,
            to,
            ..
        } => {
            format!("uses-type {from_file}:{from_line} -> {to}")
        }
        Edge::Implements {
            from_file,
            from_line,
            to,
            ..
        } => {
            format!("implements {from_file}:{from_line} -> {to}")
        }
        Edge::Ambiguous {
            origin,
            from_file,
            from_line,
            raw,
            candidates,
            ..
        } => {
            let ids: Vec<&str> = candidates.iter().map(|c| c.id.as_str()).collect();
            format!("ambiguous({origin}) {from_file}:{from_line} {raw} {ids:?}")
        }
        other => format!("{other:?}"),
    }
}

/// Only the edges that land on `to`.
fn edges_to(files: &[(&str, &str)], to: &str) -> Vec<String> {
    edges_for(files)
        .into_iter()
        .filter(|e| e.ends_with(&format!("-> {to}")))
        .collect()
}

mod bind;
mod cost;
mod crates;
mod emit;
mod macro_scope;
mod modtree;
mod opaque_globs;
mod path_modules;
mod scope;
