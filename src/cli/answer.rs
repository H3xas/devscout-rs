// The shared shape every verb's answer funnels through: the zero-hit exit
// code and its stderr note, the freshness warning, the two ambiguous-outcome
// renderers, the not-found fallback, and the one telemetry call every
// resolved query makes.

use std::io::Write;
use std::path::Path;
use std::time::Instant;

use crate::freshness::{self, FreshnessState};
use crate::graph;
use crate::manifest;
use crate::query;
use crate::query::json::J;
use crate::suggest;
use crate::telemetry;

use super::root::require_repo;

// A zero-hit answer is neither success nor failure: the index was read and the
// answer is empty. Distinct from 1 (environment, refusal) and 2 (usage) so a
// caller can branch on it instead of rephrasing the query.
pub(crate) const EXIT_NO_RESULT: i32 = 3;

// These four notes go to STDERR, never stdout, so a zero hit leaves stdout
// exactly as empty as it was -- a caller parsing stdout sees no difference, and
// the note is advice for a human reader.
pub(crate) const ZERO_HIT_FIND: &str = "devscout find: zero hits — the manifest was searched and nothing matched. Not an error; fall back to text search (rg/grep) rather than rephrasing.";
pub(crate) const ZERO_HIT_REFS: &str = "devscout refs: zero hits — the graph was searched and no symbol matched. Not an error; fall back to text search (rg/grep) rather than rephrasing.";
pub(crate) const ZERO_HIT_READ: &str = "devscout read: zero hits — the graph was searched and no symbol matched. Not an error; fall back to text search (rg/grep) rather than rephrasing.";
pub(crate) const ZERO_HIT_IMPACT: &str = "devscout impact: zero hits — the graph was searched and no affected file came back. Not an error; fall back to text search (rg/grep) rather than rephrasing.";
pub(crate) const ZERO_HIT_TESTS: &str = "devscout tests: zero hits — the graph was searched and no symbol matched. Not an error; fall back to text search (rg/grep) rather than rephrasing.";

// The one place a zero-hit line is emitted. The "did you mean" candidates extend
// that same note rather than adding a second one; `query` carries the name the
// caller asked for on the two verbs that offer them and is `None` on the two that
// do not. A `None` note is a verb declining the advice: its answer resolved and
// is merely empty, so pointing at a text search would send the caller after
// something the graph has already answered. Broken-pipe errors are swallowed for
// the same reason `print_out` swallows them.
pub(crate) fn emit_zero_hit_note(code: i32, note: Option<&str>, cwd: &Path, query: Option<&str>) {
    let Some(note) = note else { return };
    if code != EXIT_NO_RESULT {
        return;
    }
    let mut text = String::from(note);
    let rows = query.map(|q| nearest_names(cwd, q)).unwrap_or_default();
    if !rows.is_empty() {
        text.push_str("\ndid you mean:");
        for row in rows {
            text.push('\n');
            text.push_str(&row);
        }
    }
    let stderr = std::io::stderr();
    let mut lock = stderr.lock();
    let _ = lock
        .write_all(text.as_bytes())
        .and_then(|()| lock.write_all(b"\n"));
}

// Query-time index freshness, `find`/`refs`/`read`/`impact`/`tests` only: `map`
// just rebuilt the index and has nothing to say about it being stale relative to
// itself. Root resolution mirrors `require_repo`'s plain climb, never
// `require_repo_for_path`'s argument-named-file fallback -- a query that only
// resolved its root through an argument gets no freshness check, silently, the
// safe default. Called BEFORE `emit_zero_hit_note` so the two lines land on
// stderr in a fixed order when both fire for the same query. Never touches
// stdout, never changes the exit code.
pub(crate) fn emit_freshness_warning(cwd: &Path) {
    let Some(root) = crate::repo::find_scout_root(cwd).or_else(|| crate::repo::find_repo_root(cwd))
    else {
        return;
    };
    let Some(warning) = manifest::freshness_warning(&root) else {
        return;
    };
    let stderr = std::io::stderr();
    let mut lock = stderr.lock();
    let _ = lock
        .write_all(warning.as_bytes())
        .and_then(|()| lock.write_all(b"\n"));
}

// The nearest names a zero-hit `find`/`refs` offers, never substituted for the
// query and never run. The graph is read here rather than carried out of the
// command that just ran: this path is reached only once that command has decided
// it has nothing to print.
fn nearest_names(cwd: &Path, query: &str) -> Vec<String> {
    let Ok(root) = require_repo(cwd) else {
        return Vec::new();
    };
    let Some(g) = graph::read_graph(&root) else {
        return Vec::new();
    };
    suggest::suggestion_lines(&g.names, query)
}

// The one JSON envelope both ambiguous answers share, so an ambiguous type and
// an ambiguous member never drift into two shapes a consumer must tell apart:
// only the candidate objects differ, each in its renderer's own vocabulary.
fn ambiguous_json(q: &str, candidates: Vec<J>) -> String {
    J::Obj(vec![
        ("schema_version", J::UInt(query::SCHEMA_VERSION)),
        (
            "outcome",
            J::Str(query::Outcome::Ambiguous.as_str().to_string()),
        ),
        ("query", J::Str(q.to_string())),
        ("candidates", J::Arr(candidates)),
    ])
    .to_json_string()
}

// Shared by the four graph-reading verbs -- the "never guess" house rule: name
// every candidate's `{id, def site, kind}` and exit 1. `--compact` is inert here
// (a candidate list has no wide form to narrow); `--json` carries the same facts
// under `member_ambiguous_out`'s own keys.
pub(crate) fn ambiguous_candidates_out(
    index: &query::GraphIndex,
    q: &str,
    ids: &[String],
    json: bool,
) -> (i32, String) {
    let mut rows: Vec<(String, &String, &graph::Def)> = ids
        .iter()
        .map(|id| {
            // Every id here was sourced from `index.by_simple_name`/
            // `by_lower_name`, both built from `index.by_id`'s own keys during
            // construction -- `index.def(id)` cannot miss. `.expect` fails loud
            // if that invariant ever broke.
            let d = index
                .def(id)
                .expect("ambiguous candidate id must resolve to a graph def");
            (format!("{id}  {}:{}  {}", d.file, d.line, d.kind), id, d)
        })
        .collect();
    // Sorting on the rendered row compares by UTF-8 byte order, which for the
    // ASCII identifier/path/kind text these rows are built from is a stable,
    // total order (the same seam resolve.rs's candidate sort documents), and
    // gives the JSON array below the order the text list prints.
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    if json {
        let candidates = rows
            .iter()
            .map(|(_, id, d)| {
                J::Obj(vec![
                    ("id", J::Str((*id).clone())),
                    ("file", J::Str(d.file.clone())),
                    ("line", J::UInt(d.line as u64)),
                    ("kind", J::Str(d.kind.clone())),
                ])
            })
            .collect();
        return (1, ambiguous_json(q, candidates));
    }
    let mut out = vec![format!(
        "ambiguous symbol \"{q}\" — {} candidates:",
        rows.len()
    )];
    out.extend(rows.into_iter().map(|(row, _, _)| row));
    (1, out.join("\n"))
}

// Renders a member-ambiguous outcome (`RefsResult::MemberAmbiguous` and its
// mirrors on `read`/`impact`/`tests`): one row per candidate, its declaring
// type, file and line -- NEVER the bare `{id, def site, kind}` list
// `ambiguous_candidates_out` renders for an ambiguous TYPE name. Both outcomes
// answer under the same JSON keys, so a caller branching on `outcome` reads
// either without knowing which kind of name it asked about.
pub(crate) fn member_ambiguous_out(
    q: &str,
    candidates: &[query::MemberCandidate],
    json: bool,
) -> (i32, String) {
    if json {
        let rows = candidates
            .iter()
            .map(|c| {
                J::Obj(vec![
                    ("owner", J::Str(c.owner.clone())),
                    ("name", J::Str(c.name.clone())),
                    ("file", J::Str(c.file.clone())),
                    ("line", J::UInt(c.line as u64)),
                ])
            })
            .collect();
        return (1, ambiguous_json(q, rows));
    }
    let mut out = vec![format!(
        "ambiguous member \"{q}\" — {} candidates:",
        candidates.len()
    )];
    for c in candidates {
        out.push(format!("{}.{}  {}:{}", c.owner, c.name, c.file, c.line));
    }
    (1, out.join("\n"))
}

// Renders a fully-unresolved query (`NotFound` on every one of `refs`/`read`/
// `impact`/`tests`): `plain` unchanged under text/`--compact` -- the exact
// bytes these verbs always printed here -- and, only under `--json`, a small
// object naming the outcome. No JSON shape existed for this case before, so
// this is additive the same way `member_ambiguous_out`'s JSON arm is.
pub(crate) fn fallback_advised_out(q: &str, json: bool, plain: String) -> (i32, String) {
    if json {
        let out = J::Obj(vec![
            ("schema_version", J::UInt(query::SCHEMA_VERSION)),
            (
                "outcome",
                J::Str(query::Outcome::FallbackAdvised.as_str().to_string()),
            ),
            ("query", J::Str(q.to_string())),
        ])
        .to_json_string();
        (EXIT_NO_RESULT, out)
    } else {
        (EXIT_NO_RESULT, plain)
    }
}

// `freshness`'s own JSON shape: `state` first (`"fresh"`/`"stale"`/
// `"unknown"`), then the fields that state alone carries. `Stale`'s
// `indexedHead`/`currentHead` mirror `freshness_warning`'s own truncated
// stderr wording in spirit but are written full-length here -- a
// programmatic consumer should not have to guess where a short hash was cut.
fn freshness_json(state: &FreshnessState) -> J {
    match state {
        FreshnessState::Fresh => J::Obj(vec![("state", J::Str("fresh".to_string()))]),
        FreshnessState::Stale {
            indexed_head,
            current_head,
            changed_files,
        } => J::Obj(vec![
            ("state", J::Str("stale".to_string())),
            ("indexedHead", J::Str(indexed_head.clone())),
            ("currentHead", J::Str(current_head.clone())),
            ("changedFiles", J::UInt(*changed_files as u64)),
        ]),
        FreshnessState::Unknown { reason } => J::Obj(vec![
            ("state", J::Str("unknown".to_string())),
            ("reason", J::Str(reason.as_str().to_string())),
        ]),
    }
}

// Splices a top-level `freshness` key onto an already-rendered `--json`
// answer, as its new last key (after `outcome`) -- string surgery on the
// finished object rather than a second pass through the `J` tree, because
// every caller here already rendered a complete, correct object of its own
// and re-building it from scratch risks a byte drift the existing builders
// do not have today. Safe because every `--json` answer this crate renders
// is exactly one top-level object (`J::Obj(...).to_json_string()`), which
// always ends in exactly one `}` and never in trailing whitespace.
fn append_freshness(out: &str, state: &FreshnessState) -> String {
    debug_assert!(
        out.ends_with('}'),
        "a --json answer must be exactly one top-level object: {out}"
    );
    let mut spliced = out.strip_suffix('}').unwrap_or(out).to_string();
    spliced.push_str(",\"freshness\":");
    spliced.push_str(&freshness_json(state).to_json_string());
    spliced.push('}');
    spliced
}

// The one telemetry call every query verb makes, taking the rendered answer
// whole: the outcome and count recorded are the ones that answer carries, so a
// record can never describe a different answer than the caller was handed.
// `json` gates the `freshness` splice: only a `--json` answer is a single
// top-level object this can safely append to, and `find` (which never
// produces one) always passes `false`. Freshness is spliced BEFORE the
// telemetry call so `result_bytes` counts what is actually printed.
pub(crate) fn finish_query(
    root: &Path,
    verb: &'static str,
    seed: &str,
    start: Instant,
    json: bool,
    answer: (i32, String, query::Outcome, usize),
) -> (i32, String) {
    let (code, mut out, outcome, candidate_count) = answer;
    if json {
        out = append_freshness(&out, &freshness::index_freshness_state(root));
    }
    telemetry::record(
        root,
        &telemetry::QueryEvent {
            verb,
            seed,
            outcome,
            candidate_count,
        },
        start.elapsed(),
        out.len(),
    );
    (code, out)
}
