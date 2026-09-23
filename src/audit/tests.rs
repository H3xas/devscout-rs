use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::assert_check::evaluate_assert;
use super::load::parse_graph;
use super::model::{DefRow, EdgeRow, Inputs, OracleRef, Tier, Unit};
use super::render::{render_json, render_text};
use super::report::TierStats;
use super::score::{score, AuditReport};
use crate::{extract, graph, resolve};

mod render;
mod scoring;
mod semantic;

// --- fixtures --------------------------------------------------------

/// The same real-extractor/real-resolver technique `resolve.rs`'s own
/// `fragments_for` test helper uses (not reusable from here -- it is
/// private to that module's `#[cfg(test)]`), serialized and re-parsed
/// into a bare `Value`: the fixture serializes with
/// `serde_json::to_string` and feeds the result through the same `Value`
/// loader the real command uses, so no test bypasses the parse path.
fn fragments_for(files: &[(&str, &str)]) -> Vec<(String, graph::Fragment)> {
    files
        .iter()
        .map(|(rel, src)| {
            (
                (*rel).to_string(),
                graph::fragment_from_extraction(&extract::extract(src)),
            )
        })
        .collect()
}

/// Not a real git repo -- `resolve_graph`'s single I/O call (`git_head`)
/// fails closed to `None` here, same as `resolve.rs`'s own helper of the
/// same name.
fn no_git_root() -> PathBuf {
    std::env::temp_dir().join("scout-audit-test-not-a-repo")
}

fn graph_value_for(files: &[(&str, &str)]) -> serde_json::Value {
    let frags = fragments_for(files);
    let g = resolve::resolve_graph(&no_git_root(), &frags);
    let text = serde_json::to_string(&g).expect("graph serializes");
    serde_json::from_str(&text).expect("graph.json round-trips through Value")
}

#[allow(clippy::too_many_arguments)]
fn oracle_ref(
    file: &str,
    start_line: usize,
    shape: &str,
    receiver_kind: &str,
    member: &str,
    target: Option<&str>,
    target_kind: Option<&str>,
    external: bool,
) -> OracleRef {
    OracleRef {
        file: file.to_string(),
        start_line,
        shape: shape.to_string(),
        receiver_kind: receiver_kind.to_string(),
        member: member.to_string(),
        target: target.map(str::to_string),
        target_kind: target_kind.map(str::to_string),
        target_file: None,
        external,
        ambiguous: false,
    }
}
