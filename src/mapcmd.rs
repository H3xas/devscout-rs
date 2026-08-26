// Assemble devscout's `map`: walk -> parallel extract -> resolve -> artifacts,
// incremental. Glues walk.rs, extract.rs, graph.rs (which calls resolve.rs
// internally) and manifest.rs into a read-plan -> parallel-reparse -> merge ->
// write cycle, with rayon doing the parse fan-out (parsing is the only part of a
// cold map slow enough to need it -- resolution itself is pure map lookups, see
// graph.rs's own doc comment on `rebuild_graph`).
//
// ## Scoped-merge behavior
//
// A scoped `map <dir>` MERGES into the manifest instead of replacing it whole. A
// naive build that only iterates the scoped walk when constructing `entries`
// would silently drop anything outside `dirs` that was in a PRIOR manifest -- a
// data-destroying footgun (a full-repo manifest built once, then narrowed by one
// `map src/` call, would permanently lose every entry outside `src/` on the next
// write). This module partitions the prior manifest by scope membership BEFORE
// touching anything (`is_within_scope`): in-scope entries go through the normal
// reuse/reparse/prune cycle; out-of-scope entries are carried through untouched,
// never counted in preserved/downgraded/added/removed/parsed_ast (this run made
// no decision about them), and appended to the written manifest after the
// in-scope entries in their original prior-manifest order. See `map_repo`'s merge
// step and `MapReport::merged_out_of_scope`.
//
// ## Content-hash reuse (`MapOptions::hash_reuse`, the binary's default)
//
// Reuse keyed on a file's content hash instead of its mtime, so a fresh
// git-worktree checkout (new mtimes, byte-identical content) does not force a
// full reparse of every C# file -- exactly the case the shared,
// git-common-dir-keyed manifest/graph was built to serve: every linked worktree
// of one repo already shares one manifest and one graph, but a worktree that
// never ran `devscout map` before would otherwise repeat the full parse cost on
// first run because its files carry the checkout's OWN mtimes, not the mtimes
// recorded by whichever worktree wrote the shared manifest first.
//
// Confined entirely to this module: when enabled, the SAME i64 slot used for
// `mtime` (and this crate's `graph::GraphFile.mtime` / `graph::OrderedMap<i64>`
// fragments-index) instead carries the first 8 bytes of the file's SHA-256
// digest, big-endian, as a cache key -- not a timestamp. This is a deliberate,
// documented repurposing of an existing i64 slot rather than a schema change (no
// new manifest or graph field): a manifest or fragments-index written in
// hash-reuse mode has NUMERICALLY MEANINGLESS `mtime` values by wall-clock
// standards, by design. Collision risk (two distinct files' 8-byte SHA-256
// prefixes colliding) is astronomically below any concern for a reuse cache key,
// which this is -- not a content-addressing guarantee. See `hashkey::cache_key`
// and `cache_key_for`.
//
// The BINARY defaults to hash reuse (`MapOptions::from_env`), with
// `SCOUT_MTIME_REUSE=1` as the escape hatch back to mtime keying.
// `MapOptions::default()` STAYS `hash_reuse: false` (mtime keying). Two
// consequences worth knowing at the call site: (1) the first `map` over a
// manifest written in the other mode cannot match any stored key, so it repays
// one full reparse and then converges; (2) alternating the two modes against the
// same repo flips the keys back and forth, costing a full reparse each crossing.
//
// ## Parallel extraction determinism
//
// rayon fans the parse-and-extract step out with `plan.par_iter().map(..)` over
// `Vec<PlanEntry>` (an `IndexedParallelIterator`), collected into a
// `Vec<Option<ParsedFile>>` that is the SAME LENGTH as `plan` -- one slot per
// plan entry, `None` for anything that isn't reparsed. `collect()` on an indexed
// rayon iterator reassembles results at their ORIGINAL index regardless of which
// worker thread finished which file first (this, not any property of thread
// scheduling, is what makes the collected `Vec`'s order deterministic). But this
// module goes one step further and does not even lean on that alone: every
// extraction result is immediately folded into a `HashMap<String, _>`
// (order-erasing) BEFORE any artifact-affecting step consumes it, and every later
// step (the manifest `entries` build, `graph::GraphFile` list, `resolve_graph`'s
// eventual input via `graph::rebuild_graph`) iterates `plan` -- i.e. `files`,
// i.e. walk order -- not the parallel result `Vec`. So artifact byte-order is
// driven exclusively by walk order; a hypothetical rayon version that scrambled
// `collect()` order would still produce byte-identical artifacts here, because
// nothing downstream reads the parallel `Vec`'s position at all.
//
// The `--refresh` flag is a no-op alias: the manifest merge with the prior run
// always happens regardless of it, and callers strip `--refresh` out of
// `scope_dirs` before calling `map_repo`. This module does not parse CLI flags at
// all -- that is cli.rs's job.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Instant, UNIX_EPOCH};

use rayon::prelude::*;

use crate::extract;
use crate::graph::{self, AnyFragment, Fragment, GraphFile};
use crate::manifest::{self, Value};
use crate::markup;
use crate::parse;
use crate::repo;
use crate::walk;

fn io_err<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::other(e.to_string())
}

// Whether `rel` is a C# source file (`.cs`).
fn is_csharp(rel: &str) -> bool {
    rel.ends_with(".cs")
}

// The files that contribute a graph fragment: C# (defs, refs, member names),
// markup (`x:Class`/`x:Name`, `.resw` keys -- names only) and TS/JS (imports,
// exported declarations, call/JSX/dispatch references). The TS/JS arm is
// `parse::ts_grammar_for`'s own extension gate rather than a second list -- a
// file this predicate calls a graph source but the reparse dispatch below cannot
// hand a grammar would be a rel `graph_files` names and `rebuild_graph` can never
// find a fragment for, which `index_is_stale`'s length check turns into a
// permanent rebuild.
fn is_graph_source(rel: &str) -> bool {
    is_csharp(rel) || markup::is_markup(rel) || parse::ts_grammar_for(rel).is_some()
}

// LOSSY UTF-8 decoding (invalid byte sequences become U+FFFD), never failing on
// bad encoding, only on a real I/O failure (missing file, permission). Both this
// crate's extraction worker and `default_purpose` read this way;
// `fs::read_to_string` is the wrong primitive here -- it is STRICT UTF-8 and
// returns `Err` on the first invalid byte, silently dropping any file with even
// one non-UTF-8 byte (e.g. a stray CP1252 em-dash inside a `//` comment: the
// lossy read substitutes one U+FFFD and produces a normal signature, while
// `fs::read_to_string` errors on it outright). Losing a file this way is not just
// a missed purpose -- `graph::index_is_stale`'s `index.len() != graph_files.len()`
// check makes the loss STICKY: a C# file the walk finds but the fragments cache
// can never hold forces `changed == true` on every subsequent run forever,
// defeating incremental reuse for the whole graph, not just this one file.
fn read_source_lossy(path: &Path) -> Option<String> {
    fs::read(path)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

// ---------------------------------------------------------------------------
// Content-hash cache key (see module header).
// ---------------------------------------------------------------------------

mod hashkey {
    use sha2::{Digest, Sha256};

    // First 8 bytes of the file's SHA-256 digest, big-endian, as an i64 -- used
    // ONLY as an opaque reuse-cache key (see module header); never exposed as a
    // content hash proper (no hex encoding kept, no full digest retained).
    pub fn cache_key(bytes: &[u8]) -> i64 {
        let digest = Sha256::digest(bytes);
        i64::from_be_bytes(digest[0..8].try_into().expect("sha256 digest is 32 bytes"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn same_bytes_produce_the_same_key() {
            assert_eq!(cache_key(b"hello"), cache_key(b"hello"));
        }

        #[test]
        fn different_bytes_produce_different_keys() {
            assert_ne!(cache_key(b"hello"), cache_key(b"world"));
        }
    }
}

/// Options for `map_repo` -- see the module header.
#[derive(Debug, Clone, Copy, Default)]
pub struct MapOptions {
    /// Content-hash reuse (module header). `MapOptions::default()` keeps `false`
    /// (mtime keying); the binary's env default is `true` -- see `from_env`.
    pub hash_reuse: bool,
}

impl MapOptions {
    /// Binary default: hash reuse ON. Exactly `SCOUT_MTIME_REUSE=1` drops back to
    /// mtime keying; anything else (including unset) keeps hash reuse. The retired
    /// opt-in `SCOUT_HASH_REUSE` is deliberately not read any more -- it named what
    /// is now the default. A free function rather than folded into `map_repo`
    /// itself so tests (and any future caller) can construct `MapOptions` directly
    /// -- deterministic, no process-env mutation shared across parallel `cargo
    /// test` threads.
    pub fn from_env() -> Self {
        MapOptions {
            hash_reuse: std::env::var("SCOUT_MTIME_REUSE").as_deref() != Ok("1"),
        }
    }
}

/// Counts + timing `devscout map`'s CLI line reports, plus the scoped-merge
/// bookkeeping. Every field but `total_manifest_entries`/`merged_out_of_scope`
/// corresponds to a term in the one-line summary; see `summary_line`.
#[derive(Debug, Clone)]
pub struct MapReport {
    /// The scope value.
    pub scope: Vec<String>,
    /// Files this run actually walked and (re)decided a manifest entry for.
    /// This is the IN-SCOPE count only; see `total_manifest_entries` for the
    /// merged total actually written to disk.
    pub scoped_file_count: usize,
    /// `scoped_file_count` + `merged_out_of_scope` -- the manifest entry count
    /// actually written this run. Equals `scoped_file_count` whenever the prior
    /// manifest had no out-of-scope entries (always true for an unscoped/full-repo
    /// run, so this only differs from `scoped_file_count` on a genuinely scoped
    /// `map <dir>` against a manifest that already covered more).
    pub total_manifest_entries: usize,
    /// Prior entries outside this run's `scope`, carried through untouched. A
    /// naive whole-manifest replacement would drop these; this count is exactly
    /// what that approach would have destroyed.
    pub merged_out_of_scope: usize,
    /// The preserved value.
    pub preserved: usize,
    /// The downgraded value.
    pub downgraded: usize,
    /// The added value.
    pub added: usize,
    /// In-scope prior entries this run's walk no longer produced -- legitimate
    /// pruning (deleted, renamed, or newly excluded by walk.rs's skip/extension
    /// rules), unaffected by the scoped merge: an out-of-scope prior entry is
    /// never a candidate for removal, merged or not.
    pub removed: usize,
    /// The parsed ast value.
    pub parsed_ast: usize,
    /// The graph rebuilt value.
    pub graph_rebuilt: bool,
    /// The graph seconds value.
    pub graph_seconds: f64,
    /// The graph def count value.
    pub graph_def_count: Option<usize>,
    /// The graph edge count value.
    pub graph_edge_count: Option<usize>,
}

impl MapReport {
    /// The one-line `out` string `devscout map` reports. On a genuinely scoped
    /// run this describes the SCOPED work (`scoped_file_count`/`scope`), which is
    /// what this run actually decided; the out-of-scope merge is a silent,
    /// additional safety property the summary line does not mention.
    pub fn summary_line(&self) -> String {
        let graph_note = match (
            self.graph_rebuilt,
            self.graph_def_count,
            self.graph_edge_count,
        ) {
            (true, Some(defs), Some(edges)) => {
                format!(
                    "graph rebuilt in {:.2}s ({defs} defs, {edges} edges)",
                    self.graph_seconds
                )
            }
            _ => "graph unchanged".to_string(),
        };
        format!(
            "mapped {} files under {} (preserved {} agent purposes, downgraded {} changed, {} new, {} removed, {} ast signatures); {graph_note}",
            self.scoped_file_count,
            self.scope.join(", "),
            self.preserved,
            self.downgraded,
            self.added,
            self.removed,
            self.parsed_ast,
        )
    }
}

// ---------------------------------------------------------------------------
// `carries_best_source` -- an unchanged cache key alone doesn't justify reuse: a
// C# or TS/JS entry written before its language had an AST path still carries a
// heuristic purpose, and reusing it would freeze the manifest on the weaker
// source forever. Reuse holds only when the stored source is already the best
// this run could produce. The ast-capable predicate is `is_csharp(rel) ||
// parse::is_ts_js(rel)`: both languages have an AST path, so both expect "ast" as
// their best source; every other extension (docs, config, ...) expects
// "heuristic".
//
// For TS/JS specifically, "ast-none" ALSO counts as best -- a
// zero-export/parse-failure file already got the AST worker's final answer
// (None), and re-tagging it "heuristic" would make this function treat it as
// not-best forever, resending it to the AST worker on every map run. C# is
// completely untouched: it never produces "ast-none" (this module's entries-build
// below only ever writes that source for a TS/JS rel), so its own branch is
// unchanged.
// ---------------------------------------------------------------------------

fn carries_best_source(entry: &Value, rel: &str) -> bool {
    let source = entry
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("heuristic");
    if source == "agent" {
        return true;
    }
    if parse::is_ts_js(rel) {
        return source == "ast" || source == "ast-none";
    }
    source == if is_csharp(rel) { "ast" } else { "heuristic" }
}

// The `mtime` field read for the reuse comparison. Under content-hash reuse this
// same JSON field instead carries a content-hash cache key (module header) -- the
// comparison is identical either way, just against a differently-sourced i64.
fn entry_cache_key(entry: &Value) -> Option<i64> {
    match entry.get("mtime") {
        Some(Value::Number(n)) => n.as_i64(),
        _ => None,
    }
}

// A reused entry with `source` defaulted to `"heuristic"`; every OTHER field
// (including any the manifest schema doesn't know about -- manifest.rs's own
// header documents the manifest as fully schema-agnostic) passes through
// untouched. If `entry` already has a `source` key, its VALUE is overwritten in
// place (position preserved); if `entry` has no `source` key, a new one is
// appended at the end.
fn with_ensured_source(entry: &Value) -> Value {
    let fields = entry.as_object().unwrap_or(&[]);
    let source = entry
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("heuristic")
        .to_string();
    let mut out: Vec<(String, Value)> = Vec::with_capacity(fields.len() + 1);
    let mut replaced = false;
    for (k, v) in fields {
        if k == "source" {
            out.push((k.clone(), Value::string(source.clone())));
            replaced = true;
        } else {
            out.push((k.clone(), v.clone()));
        }
    }
    if !replaced {
        out.push(("source".to_string(), Value::string(source)));
    }
    Value::Object(out)
}

// Does `rel` (a manifest key, `/`-joined repo-relative path) fall under one of
// `scope`'s directories? Mirrors the SAME containment `list_source_files` uses to
// decide what to walk (`"."` covers everything; otherwise `dir` itself or
// anything under `dir/`) -- so a prior entry is "in scope" exactly when THIS
// run's walk could have produced it, independent of whether it actually did. That
// independence is the whole point: it is what lets `map_repo` tell "genuinely
// deleted (in scope, not in this run's files)" apart from "never looked at this
// run (out of scope entirely)" -- a distinction a plain "is this rel in
// `entries`" test cannot make, because that is true for neither case.
fn is_within_scope(rel: &str, scope: &[String]) -> bool {
    scope
        .iter()
        .any(|d| d == "." || rel == d || rel.starts_with(&format!("{d}/")))
}

// mtime keying (`hash_reuse == false`): the file's modification time in
// milliseconds, `0` on any stat failure (caught, not propagated -- a file that
// vanished between the walk and the stat is not fatal to the whole run). The
// value is computed as a double (`sec * 1000 + nsec / 1e6`) and then floored,
// deliberately: near-millisecond-boundary nanosecond values can round UP in that
// float math, and reproducing that exact pipeline keeps the cache key stable
// against manifests keyed the same way.
//
// content-hash reuse (`hash_reuse == true`): the file's content-hash cache key
// (module header) instead, `0` on any read failure -- same fail-open shape.
fn cache_key_for(root: &Path, rel: &str, opts: &MapOptions) -> i64 {
    let abs = root.join(rel);
    if opts.hash_reuse {
        match fs::read(&abs) {
            Ok(bytes) => hashkey::cache_key(&bytes),
            Err(_) => 0,
        }
    } else {
        match fs::metadata(&abs).and_then(|m| m.modified()) {
            Ok(t) => match t.duration_since(UNIX_EPOCH) {
                Ok(d) => {
                    let ms = (d.as_secs() as f64) * 1000.0 + f64::from(d.subsec_nanos()) / 1e6;
                    ms.floor() as i64
                }
                Err(_) => 0,
            },
            Err(_) => 0,
        }
    }
}

struct PlanEntry {
    rel: String,
    cache_key: i64,
    prior_entry: Option<Value>,
    reusable: bool,
}

// One reparsed file's result, tagged by which grammar produced it -- the C#
// branch (purpose + graph fragment, via `extract::extract`) or the TS/JS branch
// (purpose + TS reference fragment, via `extract::extract_ts_file`), from the
// same parse. Kept as an enum rather than one widened struct so a TS/JS result
// can never accidentally reach `graph::fragment_from_extraction` below, and vice
// versa.
enum ParsedFile {
    CSharp(String, extract::Extraction),
    // The purpose AND the reference fragment, off ONE parse. The purpose is
    // `None` for a zero-export file (the heuristic text stands); the fragment is
    // written either way, for the same reason the C# branch writes an empty
    // fragment -- a rel absent from the cache makes `index_is_stale` see a
    // permanent mismatch and rebuild on every run.
    TsJs(String, Option<String>, extract::TsFragment),
    // A markup or resource file: a fragment carrying names and nothing else, and
    // no purpose (the heuristic one stands).
    Markup(String, Fragment),
}

/// Runs `devscout map`, assembled from walk.rs (walk), extract.rs (extract),
/// graph.rs (resolve + graph) and manifest.rs. `scope_dirs` is the CLI's
/// positional dir arguments with `--refresh` already stripped (module header) --
/// an empty slice means "no explicit scope".
pub fn map_repo(root: &Path, scope_dirs: &[String], opts: MapOptions) -> io::Result<MapReport> {
    let scope = manifest::scope_for(
        root,
        if scope_dirs.is_empty() {
            None
        } else {
            Some(scope_dirs)
        },
    )
    .map_err(io_err)?;
    let files = walk::list_source_files(root, &scope)?;

    // A missing manifest, or one with no `entries` object, both collapse to "no
    // prior entries" (no error either way; `find_in_manifest`'s stricter
    // behavior on a missing `entries` key does NOT apply here -- `map` never
    // errors on this, only `find` does).
    let prior_manifest = manifest::read_manifest(root).map_err(io_err)?;
    let prior_entries: Vec<(String, Value)> = prior_manifest
        .as_ref()
        .and_then(|m| m.get("entries"))
        .and_then(Value::as_object)
        .map(|s| s.to_vec())
        .unwrap_or_default();
    let prior_index: HashMap<String, Value> = prior_entries.iter().cloned().collect();

    // The cheap mtime-only index (`graph::read_fragments_index`) -- read once,
    // reused for every file's fragment-freshness check below AND for the
    // `changed` decision further down, so even a fully unchanged run reads it
    // only once.
    let graph_index = graph::read_fragments_index(root);

    let plan: Vec<PlanEntry> = files
        .iter()
        .map(|rel| {
            let cache_key = cache_key_for(root, rel, &opts);
            let fragment_ok =
                !is_graph_source(rel) || graph_index.get(rel.as_str()) == Some(&cache_key);
            let prior_entry = prior_index.get(rel.as_str()).cloned();
            let reusable = prior_entry
                .as_ref()
                .map(|e| {
                    entry_cache_key(e) == Some(cache_key)
                        && carries_best_source(e, rel)
                        && fragment_ok
                })
                .unwrap_or(false);
            PlanEntry {
                rel: rel.clone(),
                cache_key,
                prior_entry,
                reusable,
            }
        })
        .collect();

    // ---- parallel reparse (rayon) --------------------------------------
    // See the module header's "Parallel extraction determinism" section:
    // `map` (not `filter`) over `plan.par_iter()` keeps this an
    // IndexedParallelIterator end to end, and the result is folded into
    // order-erasing HashMaps immediately below, before anything
    // artifact-affecting reads it.
    //
    // A reparse candidate is C#, markup or TS/JS -- each file is dispatched to
    // the C# branch (purpose + graph fragment) or the TS/JS branch (purpose + TS
    // reference fragment) on the SAME parse. Both branches produce a fragment;
    // which SHAPE it is decides which resolver sees it, and that routing is the
    // `AnyFragment` tag, never this dispatch.
    let per_file: Vec<Option<ParsedFile>> = plan
        .par_iter()
        .map(|p| {
            if p.reusable {
                return None;
            }
            if is_csharp(&p.rel) {
                return read_source_lossy(&root.join(&p.rel))
                    .map(|src| ParsedFile::CSharp(p.rel.clone(), extract::extract(&src)));
            }
            // Markup never reaches a grammar -- the scan is a line walk -- but it
            // obeys the same reuse decision every other graph source obeys, so it
            // is dispatched from the same place.
            if markup::is_markup(&p.rel) {
                return graph::markup_fragment(root, &p.rel)
                    .map(|f| ParsedFile::Markup(p.rel.clone(), f));
            }
            let grammar = parse::ts_grammar_for(&p.rel)?;
            // The leading-comment prefix rides inside `extract_ts_file` (not the
            // pure `extract_ts_purpose`): for every TS/JS file that yields an AST
            // purpose it re-checks `default_purpose_detailed` and prefixes the
            // leading comment text when that heuristic match was comment-derived.
            // The same call returns the reference fragment off that one parse, and
            // a parse failure drops the file out of BOTH outputs.
            let src = read_source_lossy(&root.join(&p.rel))?;
            let ts = extract::extract_ts_file(root, &p.rel, &src, grammar)?;
            Some(ParsedFile::TsJs(p.rel.clone(), ts.purpose, ts.fragment))
        })
        .collect();

    let mut purposes: HashMap<String, String> = HashMap::new();
    let mut fresh_fragments: HashMap<String, AnyFragment> = HashMap::new();
    for parsed in per_file.into_iter().flatten() {
        match parsed {
            ParsedFile::CSharp(rel, extraction) => {
                if let Some(p) = &extraction.purpose {
                    purposes.insert(rel.clone(), p.clone());
                }
                // Cached even when the purpose is None (no namespace-level types):
                // skipping empty fragments here would make `index_is_stale`'s "did
                // the C# set change" check see a permanent mismatch for genuinely
                // type-free files, forcing a rebuild on every run even when nothing
                // changed.
                fresh_fragments.insert(
                    rel,
                    AnyFragment::Cs(graph::fragment_from_extraction(&extraction)),
                );
            }
            ParsedFile::Markup(rel, fragment) => {
                fresh_fragments.insert(rel, AnyFragment::Cs(fragment));
            }
            ParsedFile::TsJs(rel, purpose, fragment) => {
                if let Some(p) = purpose {
                    purposes.insert(rel.clone(), p);
                }
                // Cached even when the file exports nothing, for the same reason
                // the C# branch caches an empty fragment: a rel absent from
                // `graph` makes the fragments-index reuse check see a permanent
                // mismatch and rebuild on every run.
                fresh_fragments.insert(rel, AnyFragment::Ts(fragment));
            }
        }
    }
    // A plan entry with no `per_file` result (unreadable file, reusable, or not a
    // reparse candidate at all -- e.g. a `.md`/`.json` file) simply has no
    // `purposes`/`fresh_fragments` entry -- the loop below falls back to
    // `default_purpose`, and `graph::rebuild_graph` drops it from the graph (a
    // file with no fragment contributes nothing).

    let mut entries: Vec<(String, Value)> = Vec::with_capacity(plan.len());
    let mut preserved = 0usize;
    let mut downgraded = 0usize;
    let mut added = 0usize;

    for p in &plan {
        if p.reusable {
            let merged = with_ensured_source(
                p.prior_entry
                    .as_ref()
                    .expect("reusable implies a prior entry exists"),
            );
            if merged.get("source").and_then(Value::as_str) == Some("agent") {
                preserved += 1;
            }
            entries.push((p.rel.clone(), merged));
            continue;
        }
        if let Some(prior) = p.prior_entry.as_ref() {
            let source = prior
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("heuristic");
            if source == "agent" {
                downgraded += 1;
            }
        } else {
            added += 1;
        }
        let value = match purposes.get(&p.rel) {
            Some(sig) => Value::object(vec![
                ("purpose", Value::string(sig.clone())),
                ("mtime", Value::number(p.cache_key)),
                ("source", Value::string("ast")),
            ]),
            None => {
                // A TS/JS file whose AST pass yielded no purpose (zero-export or
                // parse failure) is already the best this worker can produce this
                // run -- "ast-none" carries the same heuristic purpose text as a
                // plain heuristic result, but (unlike "heuristic")
                // `carries_best_source` above recognises it as best-obtainable for
                // TS/JS, so an unchanged mtime is reused on the next map instead of
                // being resent for reparse. C# is untouched -- it still gets
                // "heuristic".
                let source = if parse::is_ts_js(&p.rel) {
                    "ast-none"
                } else {
                    "heuristic"
                };
                Value::object(vec![
                    (
                        "purpose",
                        Value::string(walk::default_purpose(root, &p.rel)),
                    ),
                    ("mtime", Value::number(p.cache_key)),
                    ("source", Value::string(source)),
                ])
            }
        };
        entries.push((p.rel.clone(), value));
    }

    // Counts BOTH freshly-reparsed AND reused entries whose source is already
    // "ast", over the (pre-merge, in-scope) `entries`.
    let parsed_ast = entries
        .iter()
        .filter(|(_, v)| v.get("source").and_then(Value::as_str) == Some("ast"))
        .count();

    // In-scope prior entries this run's walk no longer produced -- see
    // `removed`'s doc comment on `MapReport`. Scoped to a block so the
    // borrow of `entries` ends before the out-of-scope merge loop mutates it.
    let removed = {
        let entries_index: HashSet<String> = entries.iter().map(|(k, _)| k.clone()).collect();
        prior_entries
            .iter()
            .filter(|entry| is_within_scope(&entry.0, &scope) && !entries_index.contains(&entry.0))
            .count()
    };

    let scoped_file_count = entries.len();

    // Scoped merge (module header): every out-of-scope prior entry is carried
    // through untouched, in its original prior-manifest order, appended after
    // this run's in-scope entries -- never counted in
    // preserved/downgraded/added/removed/parsed_ast above.
    let mut merged_out_of_scope = 0usize;
    for entry in &prior_entries {
        if !is_within_scope(&entry.0, &scope) {
            entries.push(entry.clone());
            merged_out_of_scope += 1;
        }
    }
    let total_manifest_entries = entries.len();
    let indexed_file_set: HashSet<String> = entries.iter().map(|(k, _)| k.clone()).collect();

    let built_at_head = manifest::git_head(root);
    let manifest_value = Value::object(vec![
        (
            "built_at_head",
            built_at_head
                .clone()
                .map(Value::string)
                .unwrap_or(Value::Null),
        ),
        (
            "scoped_dirs",
            Value::array(scope.iter().cloned().map(Value::string).collect()),
        ),
        ("entries", Value::Object(entries)),
    ]);
    manifest::write_manifest(root, &manifest_value)?;

    // Index-freshness sidecar, written every successful map run, the same cadence
    // manifest.json's own built_at_head refreshes on.
    let dirty = manifest::is_working_tree_dirty(root, &scope);
    let dirty_indexed_files = manifest::dirty_indexed_files_at(root, &scope, &indexed_file_set);
    manifest::write_index_state(
        root,
        built_at_head,
        dirty,
        &dirty_indexed_files,
        total_manifest_entries as i64,
    )?;

    // Clear the `refresh-needed` flag if present.
    let flag = repo::scout_dir(root).join("refresh-needed");
    if flag.exists() {
        fs::remove_file(&flag)?;
    }

    let graph_files: Vec<GraphFile> = plan
        .iter()
        .filter(|p| is_graph_source(&p.rel))
        .map(|p| GraphFile {
            rel: p.rel.clone(),
            mtime: p.cache_key,
        })
        .collect();
    // `graph::index_is_stale` decides whether the graph must be rebuilt (any
    // graph file's cache key changed, or the set of graph files changed size),
    // reused here rather than re-implemented.
    let changed = graph::index_is_stale(&graph_index, &graph_files);

    let graph_start = Instant::now();
    let outcome = graph::rebuild_graph(root, &graph_files, &fresh_fragments, changed)?;
    let graph_seconds = graph_start.elapsed().as_secs_f64();

    let (graph_rebuilt, graph_def_count, graph_edge_count) = match &outcome {
        graph::RebuildOutcome::Rebuilt(g) => (true, Some(g.defs.len()), Some(g.edges.len())),
        graph::RebuildOutcome::NotRebuilt => (false, None, None),
    };

    Ok(MapReport {
        scope,
        scoped_file_count,
        total_manifest_entries,
        merged_out_of_scope,
        preserved,
        downgraded,
        added,
        removed,
        parsed_ast,
        graph_rebuilt,
        graph_seconds,
        graph_def_count,
        graph_edge_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "scout-mapcmd-test-{label}-{}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(path, contents).expect("write file");
    }

    // -- is_within_scope --------------------------------------------------

    #[test]
    fn dot_scope_covers_everything() {
        assert!(is_within_scope("src/a.ts", &[".".to_string()]));
        assert!(is_within_scope("a.ts", &[".".to_string()]));
    }

    #[test]
    fn dir_scope_covers_itself_and_nested_paths_only() {
        let scope = vec!["src/Foo".to_string()];
        assert!(is_within_scope("src/Foo/A.cs", &scope));
        assert!(is_within_scope("src/Foo/Sub/B.cs", &scope));
        assert!(!is_within_scope("src/Bar/C.cs", &scope));
        // A same-prefixed SIBLING directory ("src/FooBar") must not match --
        // this is why the check requires the "/" boundary, not a bare
        // `starts_with(d)`.
        assert!(!is_within_scope("src/FooBar/D.cs", &scope));
    }

    #[test]
    fn multiple_scope_dirs_union() {
        let scope = vec!["src".to_string(), "docs".to_string()];
        assert!(is_within_scope("src/a.ts", &scope));
        assert!(is_within_scope("docs/readme.md", &scope));
        assert!(!is_within_scope("config/settings.json", &scope));
    }

    // -- carries_best_source ------------------------------------------------

    #[test]
    fn agent_source_always_carries() {
        let e = Value::object(vec![("source", Value::string("agent"))]);
        assert!(carries_best_source(&e, "a.cs"));
        assert!(carries_best_source(&e, "a.ts"));
    }

    // `carries_best_source`'s "ast-capable" predicate is `is_csharp(rel) ||
    // parse::is_ts_js(rel)` -- both languages have an AST path, so both expect
    // "ast" as their best source. `a.json`/`a.md`/etc. (neither predicate) expect
    // "heuristic".
    #[test]
    fn ast_source_carries_for_csharp_and_ts_js() {
        let e = Value::object(vec![("source", Value::string("ast"))]);
        assert!(carries_best_source(&e, "a.cs"));
        assert!(carries_best_source(&e, "a.ts"));
        assert!(!carries_best_source(&e, "a.json"));
    }

    #[test]
    fn heuristic_source_carries_only_for_non_ast_capable_extensions() {
        let e = Value::object(vec![("source", Value::string("heuristic"))]);
        assert!(!carries_best_source(&e, "a.cs"));
        assert!(!carries_best_source(&e, "a.ts"));
        assert!(carries_best_source(&e, "a.json"));
    }

    #[test]
    fn missing_source_defaults_to_heuristic() {
        let e = Value::object(vec![("purpose", Value::string("x"))]);
        assert!(carries_best_source(&e, "a.json"));
        assert!(!carries_best_source(&e, "a.cs"));
        assert!(!carries_best_source(&e, "a.ts"));
    }

    // "ast-none" carries the best source for TS/JS (no more forced resend on
    // every rerun), but is NOT a valid best source for C# -- that path never
    // produces it.
    #[test]
    fn ast_none_source_carries_for_ts_js_but_not_for_csharp_or_plain_files() {
        let e = Value::object(vec![("source", Value::string("ast-none"))]);
        assert!(carries_best_source(&e, "a.ts"));
        assert!(carries_best_source(&e, "a.tsx"));
        assert!(carries_best_source(&e, "a.js"));
        assert!(
            !carries_best_source(&e, "a.cs"),
            "C# never produces ast-none; an entry claiming it must still be reparsed"
        );
        assert!(!carries_best_source(&e, "a.json"));
    }

    // -- with_ensured_source: source-field position semantics --------------

    #[test]
    fn ensured_source_is_appended_when_absent() {
        let e = Value::object(vec![
            ("purpose", Value::string("p")),
            ("mtime", Value::number(1)),
        ]);
        let out = with_ensured_source(&e);
        let fields = out.as_object().unwrap();
        assert_eq!(fields.last().unwrap().0, "source");
        assert_eq!(out.get("source").and_then(Value::as_str), Some("heuristic"));
    }

    #[test]
    fn ensured_source_overwrites_in_place_preserving_position() {
        let e = Value::object(vec![
            ("source", Value::string("agent")),
            ("purpose", Value::string("p")),
            ("mtime", Value::number(1)),
        ]);
        let out = with_ensured_source(&e);
        let fields = out.as_object().unwrap();
        // Position preserved (still first), value untouched (already "agent").
        assert_eq!(fields[0].0, "source");
        assert_eq!(out.get("source").and_then(Value::as_str), Some("agent"));
    }

    #[test]
    fn ensured_source_null_collapses_to_heuristic_in_place() {
        let e = Value::object(vec![
            ("purpose", Value::string("p")),
            ("source", Value::Null),
        ]);
        let out = with_ensured_source(&e);
        let fields = out.as_object().unwrap();
        assert_eq!(fields[1].0, "source"); // position preserved (was 2nd key)
        assert_eq!(out.get("source").and_then(Value::as_str), Some("heuristic"));
    }

    // -- map_repo: full run, reuse, scoped merge -----------------------------

    #[test]
    fn full_run_builds_entries_with_expected_sources() {
        let root = temp_dir("full");
        write_file(
            &root.join("src/A.cs"),
            "namespace Fixtures.MapCmd\n{\n    public class A\n    {\n    }\n}\n",
        );
        // This composes a purpose (`const x`) via the TS AST path instead of
        // falling back to the heuristic -- source is "ast".
        write_file(
            &root.join("src/note.ts"),
            "// a helper\nexport const x = 1;\n",
        );
        // A file with no exported top-level declaration still degrades to
        // the heuristic purpose, same as a C# file with no namespace-level
        // type -- proves the AST-vs-heuristic branch is still live, not
        // just "every TS/JS file is always ast now".
        write_file(
            &root.join("src/internal.ts"),
            "function helper() { return 1; }\n",
        );

        let report = map_repo(&root, &[], MapOptions::default()).unwrap();
        assert_eq!(report.scoped_file_count, 3);
        assert_eq!(report.total_manifest_entries, 3);
        assert_eq!(report.merged_out_of_scope, 0);
        assert_eq!(report.added, 3);
        assert_eq!(
            report.parsed_ast, 2,
            "A.cs and note.ts both compose a purpose; internal.ts has no exported declaration"
        );
        assert!(report.graph_rebuilt);
        // `A.cs`'s class AND `note.ts`'s exported `const x` -- TS/JS files
        // contribute graph defs. `internal.ts` exports nothing, so it still
        // contributes a cached fragment and no def.
        assert_eq!(
            report.graph_def_count,
            Some(2),
            "A.cs's class plus note.ts's exported const"
        );

        let manifest = manifest::read_manifest(&root).unwrap().unwrap();
        let entries = manifest.get("entries").unwrap().as_object().unwrap();
        let a = entries
            .iter()
            .find(|(k, _)| k == "src/A.cs")
            .unwrap()
            .1
            .clone();
        assert_eq!(a.get("source").and_then(Value::as_str), Some("ast"));
        let note = entries
            .iter()
            .find(|(k, _)| k == "src/note.ts")
            .unwrap()
            .1
            .clone();
        assert_eq!(note.get("source").and_then(Value::as_str), Some("ast"));
        // note.ts's first line ("// a helper") IS comment-derived under
        // `default_purpose_detailed`, so its AST purpose carries the
        // leading-comment hybrid prefix.
        assert_eq!(
            note.get("purpose").and_then(Value::as_str),
            Some("a helper — const x")
        );
        let internal = entries
            .iter()
            .find(|(k, _)| k == "src/internal.ts")
            .unwrap()
            .1
            .clone();
        // A zero-export TS/JS file is tagged "ast-none", not "heuristic" --
        // carries the same heuristic purpose text, but is recognised as
        // best-obtainable by `carries_best_source` so an unchanged rerun reuses
        // it instead of resending it.
        assert_eq!(
            internal.get("source").and_then(Value::as_str),
            Some("ast-none")
        );
        assert_eq!(
            internal.get("purpose").and_then(Value::as_str),
            Some("function helper() { return 1; }")
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ts_purpose_upgrades_from_heuristic_and_then_reuses_unchanged() {
        // TS reuse semantics: a TS entry with source "heuristic" upgrades to
        // "ast" on the first map with the AST path; an immediate unchanged rerun
        // then reuses it (no reparse, counted in `parsed_ast` since it already
        // carries "ast").
        let root = temp_dir("ts-reuse");
        write_file(&root.join("src/bar.ts"), "export function bar() {}\n");

        let first = map_repo(&root, &[], MapOptions::default()).unwrap();
        assert_eq!(first.added, 1);
        assert_eq!(first.parsed_ast, 1);
        let manifest = manifest::read_manifest(&root).unwrap().unwrap();
        let entries = manifest.get("entries").unwrap().as_object().unwrap();
        let bar = entries
            .iter()
            .find(|(k, _)| k == "src/bar.ts")
            .unwrap()
            .1
            .clone();
        assert_eq!(bar.get("source").and_then(Value::as_str), Some("ast"));
        assert_eq!(
            bar.get("purpose").and_then(Value::as_str),
            Some("function bar")
        );

        let second = map_repo(&root, &[], MapOptions::default()).unwrap();
        assert_eq!(second.added, 0);
        assert_eq!(second.downgraded, 0);
        assert_eq!(
            second.parsed_ast, 1,
            "reused ast-source TS entry still counts as ast on an unchanged rerun"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unchanged_second_run_reuses_everything() {
        let root = temp_dir("reuse");
        write_file(
            &root.join("src/A.cs"),
            "namespace Fixtures.MapCmd\n{\n    public class A\n    {\n    }\n}\n",
        );

        let first = map_repo(&root, &[], MapOptions::default()).unwrap();
        assert_eq!(first.added, 1);

        let second = map_repo(&root, &[], MapOptions::default()).unwrap();
        assert_eq!(second.added, 0);
        assert_eq!(second.downgraded, 0);
        assert_eq!(second.removed, 0);
        assert_eq!(second.parsed_ast, 1, "reused entry keeps its ast source");
        assert!(
            !second.graph_rebuilt,
            "unchanged C# set must not rebuild the graph"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn deleted_file_is_pruned_and_counted_removed() {
        let root = temp_dir("prune");
        write_file(
            &root.join("src/A.cs"),
            "namespace Fixtures.MapCmd { public class A {} }\n",
        );
        write_file(
            &root.join("src/B.cs"),
            "namespace Fixtures.MapCmd { public class B {} }\n",
        );
        map_repo(&root, &[], MapOptions::default()).unwrap();

        fs::remove_file(root.join("src/B.cs")).unwrap();
        let report = map_repo(&root, &[], MapOptions::default()).unwrap();
        assert_eq!(report.removed, 1);
        assert_eq!(report.total_manifest_entries, 1);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scoped_run_merges_out_of_scope_entries() {
        let root = temp_dir("scoped-merge");
        write_file(
            &root.join("src/Foo/A.cs"),
            "namespace Fixtures.MapCmd.Foo { public class A {} }\n",
        );
        write_file(
            &root.join("src/Bar/C.ts"),
            "// bar helper\nexport const bar = 1;\n",
        );
        write_file(&root.join("docs/readme.md"), "# Fixtures Map Demo\n");

        let full = map_repo(&root, &[], MapOptions::default()).unwrap();
        assert_eq!(full.total_manifest_entries, 3);

        let scoped = map_repo(&root, &["src/Foo".to_string()], MapOptions::default()).unwrap();
        assert_eq!(scoped.scope, vec!["src/Foo".to_string()]);
        assert_eq!(
            scoped.scoped_file_count, 1,
            "only src/Foo/A.cs is walked this run"
        );
        assert_eq!(
            scoped.merged_out_of_scope, 2,
            "src/Bar/C.ts and docs/readme.md preserved, not destroyed"
        );
        assert_eq!(scoped.total_manifest_entries, 3);
        assert_eq!(scoped.removed, 0, "nothing in scope was actually deleted");

        let manifest = manifest::read_manifest(&root).unwrap().unwrap();
        let entries = manifest.get("entries").unwrap().as_object().unwrap();
        assert_eq!(entries.len(), 3);
        assert!(
            entries.iter().any(|(k, _)| k == "src/Bar/C.ts"),
            "out-of-scope entry must survive a scoped map"
        );
        assert!(
            entries.iter().any(|(k, _)| k == "docs/readme.md"),
            "out-of-scope entry must survive a scoped map"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scoped_run_still_prunes_in_scope_deletions() {
        let root = temp_dir("scoped-prune");
        write_file(
            &root.join("src/Foo/A.cs"),
            "namespace Fixtures.MapCmd.Foo { public class A {} }\n",
        );
        write_file(
            &root.join("src/Foo/B.cs"),
            "namespace Fixtures.MapCmd.Foo { public class B {} }\n",
        );
        write_file(
            &root.join("src/Bar/C.ts"),
            "// bar\nexport const bar = 1;\n",
        );
        map_repo(&root, &[], MapOptions::default()).unwrap();

        fs::remove_file(root.join("src/Foo/B.cs")).unwrap();
        let scoped = map_repo(&root, &["src/Foo".to_string()], MapOptions::default()).unwrap();
        // In-scope deletion IS pruned (not merge-preserved) -- the scoped merge
        // only protects entries this run never looked at, not entries it looked
        // at and found gone.
        assert_eq!(scoped.removed, 1);
        assert_eq!(
            scoped.merged_out_of_scope, 1,
            "src/Bar/C.ts still preserved"
        );
        assert_eq!(scoped.total_manifest_entries, 2);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn invalid_utf8_byte_is_read_lossily_not_dropped() {
        // Regression: a stray CP1252 byte (0x97, em-dash) inside a `//` comment
        // must not cause the whole file to be silently excluded from extraction
        // (see `read_source_lossy`'s doc comment).
        let root = temp_dir("lossy-utf8");
        let path = root.join("src/Weird.cs");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = b"namespace Fixtures.MapCmd\n{\n    // note \x97 em dash\n    public class Weird\n    {\n    }\n}\n".to_vec();
        assert!(
            std::str::from_utf8(&bytes).is_err(),
            "fixture must actually contain an invalid UTF-8 byte"
        );
        fs::write(&path, &bytes).unwrap();
        bytes.clear(); // silence "unused" if the assert above is ever removed

        let report = map_repo(&root, &[], MapOptions::default()).unwrap();
        assert_eq!(report.added, 1);
        assert_eq!(
            report.parsed_ast, 1,
            "the file must still be AST-extracted despite the bad byte"
        );
        assert!(report.graph_rebuilt);
        assert_eq!(report.graph_def_count, Some(1));

        // The regression's OTHER symptom: index_is_stale's length check
        // must not be permanently tripped by a dropped file -- an immediate
        // unchanged rerun must skip the graph rebuild.
        let second = map_repo(&root, &[], MapOptions::default()).unwrap();
        assert!(
            !second.graph_rebuilt,
            "an unchanged rerun must reuse, not be permanently stale"
        );

        fs::remove_dir_all(&root).ok();
    }

    // -- content-hash reuse --------------------------------------------

    #[test]
    fn hash_reuse_survives_an_mtime_only_touch() {
        let root = temp_dir("hash-reuse");
        let opts = MapOptions { hash_reuse: true };
        let content = "namespace Fixtures.MapCmd { public class A {} }\n";
        write_file(&root.join("src/A.cs"), content);
        let first = map_repo(&root, &[], opts).unwrap();
        assert_eq!(first.added, 1);
        assert!(first.graph_rebuilt);

        // Rewrite with IDENTICAL content -- a real mtime bump (a fresh
        // worktree checkout's own failure mode), but the content hash is
        // unchanged.
        std::thread::sleep(std::time::Duration::from_millis(10));
        write_file(&root.join("src/A.cs"), content);

        let second = map_repo(&root, &[], opts).unwrap();
        assert_eq!(second.added, 0);
        assert_eq!(second.downgraded, 0);
        assert!(
            !second.graph_rebuilt,
            "same content hash must reuse, not reparse"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn hash_reuse_reparses_on_real_content_change() {
        let root = temp_dir("hash-reparse");
        let opts = MapOptions { hash_reuse: true };
        write_file(
            &root.join("src/A.cs"),
            "namespace Fixtures.MapCmd { public class A {} }\n",
        );
        map_repo(&root, &[], opts).unwrap();

        write_file(
            &root.join("src/A.cs"),
            "namespace Fixtures.MapCmd { public class A { public void M() {} } }\n",
        );
        let second = map_repo(&root, &[], opts).unwrap();
        assert!(second.graph_rebuilt, "changed content hash must reparse");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn mtime_default_mode_ignores_content_and_keys_on_mtime_only() {
        // Sanity check that the two modes are actually distinct: rewriting
        // identical content under DEFAULT (mtime) mode still forces a
        // reparse if the mtime moved, unlike hash_reuse above.
        let root = temp_dir("mtime-default");
        let content = "namespace Fixtures.MapCmd { public class A {} }\n";
        write_file(&root.join("src/A.cs"), content);
        map_repo(&root, &[], MapOptions::default()).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));
        write_file(&root.join("src/A.cs"), content);
        let second = map_repo(&root, &[], MapOptions::default()).unwrap();
        // mtime moved -> not reusable -> reparsed -> graph still has the
        // same single def, but the fragments-index mtime must have moved,
        // which rebuild_graph treats as "changed".
        assert!(
            second.graph_rebuilt,
            "default mode has no content-awareness -- an mtime bump alone reparses"
        );

        fs::remove_dir_all(&root).ok();
    }

    // -- ast-none manifest + reuse ------------------------------------------

    #[test]
    fn zero_export_ts_file_is_tagged_ast_none_not_heuristic() {
        let root = temp_dir("ast-none-tag");
        write_file(
            &root.join("src/internal.ts"),
            "function helper() { return 1; }\n",
        );

        let report = map_repo(&root, &[], MapOptions::default()).unwrap();
        assert_eq!(report.added, 1);
        assert_eq!(
            report.parsed_ast, 0,
            "ast-none never counts toward parsed_ast"
        );

        let manifest = manifest::read_manifest(&root).unwrap().unwrap();
        let entries = manifest.get("entries").unwrap().as_object().unwrap();
        let entry = entries
            .iter()
            .find(|(k, _)| k == "src/internal.ts")
            .unwrap()
            .1
            .clone();
        assert_eq!(
            entry.get("source").and_then(Value::as_str),
            Some("ast-none")
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unchanged_rerun_does_not_recompute_ast_none_purposes() {
        // The resend trap: plant a sentinel purpose a fresh recompute could never
        // produce, at the SAME cache key/source ("ast-none"). If the file is
        // resent for recompute on the next map (ast-none miscategorised as
        // not-best), the real heuristic text overwrites the sentinel; correct
        // reuse leaves it intact. A byte-identical manifest.json alone would NOT
        // prove this -- a resent file recomputes to the same deterministic text --
        // so this is the direct evidence for "rerun resends 0 TS files".
        let root = temp_dir("ast-none-resend-trap");
        write_file(
            &root.join("src/internal.ts"),
            "function helper() { return 1; }\n",
        );
        map_repo(&root, &[], MapOptions::default()).unwrap();

        let manifest = manifest::read_manifest(&root).unwrap().unwrap();
        let built_at_head = manifest
            .get("built_at_head")
            .cloned()
            .unwrap_or(Value::Null);
        let scoped_dirs = manifest
            .get("scoped_dirs")
            .cloned()
            .unwrap_or_else(|| Value::array(vec![]));
        let mut entries: Vec<(String, Value)> = manifest
            .get("entries")
            .and_then(Value::as_object)
            .unwrap()
            .to_vec();
        for (k, v) in entries.iter_mut() {
            if k == "src/internal.ts" {
                assert_eq!(
                    v.get("source").and_then(Value::as_str),
                    Some("ast-none"),
                    "precondition"
                );
                let cache_key = v.get("mtime").cloned().expect("mtime present");
                *v = Value::object(vec![
                    ("purpose", Value::string("__SENTINEL_DO_NOT_RECOMPUTE__")),
                    ("mtime", cache_key),
                    ("source", Value::string("ast-none")),
                ]);
            }
        }
        manifest::write_manifest(
            &root,
            &Value::object(vec![
                ("built_at_head", built_at_head),
                ("scoped_dirs", scoped_dirs),
                ("entries", Value::Object(entries)),
            ]),
        )
        .unwrap();

        let second = map_repo(&root, &[], MapOptions::default()).unwrap();
        assert_eq!(second.added, 0);
        assert_eq!(second.downgraded, 0);

        let after = manifest::read_manifest(&root).unwrap().unwrap();
        let after_entries = after.get("entries").and_then(Value::as_object).unwrap();
        let reused = after_entries
            .iter()
            .find(|(k, _)| k == "src/internal.ts")
            .unwrap()
            .1
            .clone();
        assert_eq!(
            reused.get("purpose").and_then(Value::as_str),
            Some("__SENTINEL_DO_NOT_RECOMPUTE__"),
            "unchanged rerun must not resend this file for recompute"
        );
        assert_eq!(
            reused.get("source").and_then(Value::as_str),
            Some("ast-none")
        );

        fs::remove_dir_all(&root).ok();
    }

    // -- non-git root (no manifest yet) ------------------------------------

    #[test]
    fn empty_dir_maps_to_zero_entries_without_erroring() {
        let root = temp_dir("empty");
        fs::create_dir_all(&root).unwrap();
        let report = map_repo(&root, &[], MapOptions::default()).unwrap();
        assert_eq!(report.scoped_file_count, 0);
        assert_eq!(report.total_manifest_entries, 0);
        // First run always writes an initial (empty) graph.json -- the graph
        // rebuild only skips when `!changed` AND a graph.json already exists; on a
        // fresh repo neither is true yet.
        assert!(report.graph_rebuilt);
        assert_eq!(report.graph_def_count, Some(0));

        // A SECOND run against the same (still empty) root has an existing,
        // unchanged (0 == 0 C# files) graph.json -- this is the case that
        // actually exercises the "skip" path.
        let second = map_repo(&root, &[], MapOptions::default()).unwrap();
        assert!(!second.graph_rebuilt);

        fs::remove_dir_all(&root).ok();
    }
}
