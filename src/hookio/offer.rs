// The read hook's one-line offer for a freshly-delivered file, and the gate
// that decides whether an offer is honest.
//
// An offer says "this file is indexed -- ask devscout for its declaration".
// That claim is only true when the index describes the very bytes the reader is
// looking at, so the gate requires all of: a record this build can certify, the
// reading worktree as its owner, the record's own HEAD, the graph identity the
// record certifies, and this file's on-disk digest against the digest table.
// The hook runs no `git status` for any of it -- the HEAD is the `rev-parse` the
// read path already ran, and the rest are two small files and one hash.

use std::fs;
use std::path::Path;

use crate::freshness::{record, RecordState};
use crate::graph::graph_json_path;
use crate::manifest::Value as JVal;
use crate::repo;

use super::jval_i64;

// Whether the read hook may offer a symbol for `file_path` in `rel`.
//
// `head` is the HEAD this read already resolved, never re-spawned here. A
// sibling worktree's index, a file edited again after it was indexed, and an
// index written by an older build all fail one of the checks and produce no
// offer.
pub(super) fn offer_permitted(root: &Path, file_path: &Path, rel: &str, head: &str) -> bool {
    let RecordState::Published(record) = record::read_record(root) else {
        return false;
    };
    let Some(root_owner) = record::owner_of(root) else {
        return false;
    };
    if root_owner != record.owner {
        return false;
    }
    // The worktree doing the reading, named by its own Git administrative
    // directory -- a linked worktree nested under a mapped checkout resolves to
    // itself here, unlike the `.scout`-ancestor root climb.
    let Some(asking) = repo::find_repo_root(file_path).and_then(|r| record::owner_of(&r)) else {
        return false;
    };
    if asking != record.owner {
        return false;
    }
    if record.indexed_head.as_deref() != Some(head) {
        return false;
    }
    if record::ArtifactIdentity::of(&graph_json_path(root)) != record.artifacts.graph {
        return false;
    }
    let Some(table) = record::read_digests(root) else {
        return false;
    };
    if table.generation != record.generation {
        return false;
    }
    let Some(expected) = table.files.get(rel) else {
        return false;
    };
    let Ok(bytes) = fs::read(file_path) else {
        return false;
    };
    record::sha256_hex(&bytes) == *expected
}

// Builds the one-line `devscout read` offer for a freshly-delivered file:
// `None` whenever there is nothing honest to offer -- no graph artifact for
// this root, no defs mapped in this file -- so those reads keep today's
// empty output exactly. The offered symbol is the def whose declaration span
// sits nearest to the range this read returned; when the payload does not
// say where the read sat (the legacy text-only response shape carries no
// line bounds at all), the fallback is the file's FIRST mapped symbol --
// noted here because it is a limitation of the payload shape, not a choice
// about the code: a whole-file read would tie-break to that same first row
// anyway.
pub(super) fn first_read_offer(root: &Path, rel: &str, input: &JVal) -> Option<String> {
    let g = crate::graph::read_graph(root)?;
    let defs: Vec<&crate::graph::Def> = g.defs.iter().filter(|d| d.file == rel).collect();
    if defs.is_empty() {
        return None;
    }

    // Requested range: what the tool_response reported about itself
    // (`file.startLine`, `file.numLines`), falling back to tool_input's
    // `offset` when the response carries no start of its own. A start with
    // no length reads as "everything from here down" (`usize::MAX` end),
    // which makes every span at-or-below the start overlap it; no bounds in
    // the payload at all reads as a whole-file read, `(1, MAX)` -- whose
    // tie-break is exactly the first-mapped-symbol fallback documented on
    // this function.
    let orig_file = input.get("tool_response").and_then(|r| r.get("file"));
    let req_start = orig_file
        .and_then(|f| f.get("startLine"))
        .and_then(jval_i64)
        .or_else(|| {
            input
                .get("tool_input")
                .and_then(|t| t.get("offset"))
                .and_then(jval_i64)
        })
        .map(|v| v.max(1) as usize);
    let num_lines = orig_file.and_then(|f| f.get("numLines")).and_then(jval_i64);
    let (rs, re) = match (req_start, num_lines) {
        (Some(s), Some(n)) => (s, s.saturating_sub(1).saturating_add(n.max(0) as usize)),
        (Some(s), None) => (s, usize::MAX),
        (None, _) => (1, usize::MAX),
    };
    nearest_def(&defs, rs, re).map(|d| offer_text(&g, d))
}

// Gap between a def's declaration span and the requested range: zero on any
// overlap, otherwise how many lines apart they sit. Ties break by earlier
// start line, then by id -- deterministic, independent of map order.
fn nearest_def<'a>(
    defs: &[&'a crate::graph::Def],
    req_start: usize,
    req_end: usize,
) -> Option<&'a crate::graph::Def> {
    defs.iter().copied().min_by(|a, b| {
        let key = |d: &crate::graph::Def| {
            let ds = d.line.max(1);
            // `0` is the "no end recorded" sentinel (TS defs); such a
            // span measures as its start line alone rather than as a
            // guessed range.
            let de = if d.end_line >= ds { d.end_line } else { ds };
            // Below the range: how far short it falls. Otherwise 0 on a true
            // overlap, and for a span past the range's end saturating_sub
            // yields the same positive gap the explicit branch did.
            let gap = if de < req_start {
                req_start - de
            } else {
                ds.saturating_sub(req_end)
            };
            (gap, ds)
        };
        key(a).cmp(&key(b)).then_with(|| a.id.cmp(&b.id))
    })
}

// The offer sentence itself. The symbol is named the way the verb can take
// it: the simple name when no other def shares it (the resolver answers a
// unique name directly), the full def id when the bare name would print an
// ambiguity -- never a guess between the two.
fn offer_text(g: &crate::graph::Graph, chosen: &crate::graph::Def) -> String {
    let ambiguous = g
        .defs
        .iter()
        .any(|d| d.id != chosen.id && d.name == chosen.name);
    let subject = if ambiguous {
        chosen.id.as_str()
    } else {
        chosen.name.as_str()
    };
    format!(
        "[devscout: this file is indexed — 'devscout read {subject}' shows its declaration span and inbound references.]"
    )
}
