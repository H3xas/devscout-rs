// The per-language half of `map`: which files are graph sources, how one file
// is read and reparsed, what counts as its best manifest source, and how the
// parallel results fold into purposes and fragments. Kept in its own sibling
// file (not inline in `mapcmd.rs`) because that file sits at its
// `tools/size-ratchet.toml` ceiling; a new language lands here, and
// `map_repo` keeps only the language-free orchestration (walk, plan, reuse,
// manifest, graph rebuild).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::extract;
use crate::graph::{self, AnyFragment, Fragment};
use crate::manifest::Value;
use crate::markup;
use crate::parse;

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
pub(super) fn is_graph_source(rel: &str) -> bool {
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
// completely untouched: it never produces "ast-none" (`fallback_source` below
// only ever gives that source for a TS/JS rel), so its own branch is
// unchanged.
// ---------------------------------------------------------------------------

pub(super) fn carries_best_source(entry: &Value, rel: &str) -> bool {
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

// One reparsed file's result, tagged by which grammar produced it -- the C#
// branch (purpose + graph fragment, via `extract::extract`) or the TS/JS branch
// (purpose + TS reference fragment, via `extract::extract_ts_file`), from the
// same parse. Kept as an enum rather than one widened struct so a TS/JS result
// can never accidentally reach `graph::fragment_from_extraction` below, and vice
// versa.
pub(super) enum ParsedFile {
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

// One non-reusable plan entry's reparse: each file is dispatched to the C#
// branch (purpose + graph fragment) or the TS/JS branch (purpose + TS
// reference fragment) on the SAME parse. Both branches produce a fragment;
// which SHAPE it is decides which resolver sees it, and that routing is the
// `AnyFragment` tag, never this dispatch.
pub(super) fn reparse(root: &Path, rel: &str) -> Option<ParsedFile> {
    if is_csharp(rel) {
        return read_source_lossy(&root.join(rel))
            .map(|src| ParsedFile::CSharp(rel.to_string(), extract::extract(&src)));
    }
    // Markup never reaches a grammar -- the scan is a line walk -- but it
    // obeys the same reuse decision every other graph source obeys, so it
    // is dispatched from the same place.
    if markup::is_markup(rel) {
        return graph::markup_fragment(root, rel).map(|f| ParsedFile::Markup(rel.to_string(), f));
    }
    let grammar = parse::ts_grammar_for(rel)?;
    // The leading-comment prefix rides inside `extract_ts_file` (not the
    // pure `extract_ts_purpose`): for every TS/JS file that yields an AST
    // purpose it re-checks `default_purpose_detailed` and prefixes the
    // leading comment text when that heuristic match was comment-derived.
    // The same call returns the reference fragment off that one parse, and
    // a parse failure drops the file out of BOTH outputs.
    let src = read_source_lossy(&root.join(rel))?;
    let ts = extract::extract_ts_file(root, rel, &src, grammar)?;
    Some(ParsedFile::TsJs(rel.to_string(), ts.purpose, ts.fragment))
}

// Folds the parallel results into order-erasing maps (see `mapcmd.rs`'s
// "Parallel extraction determinism" header): the AST purposes by rel, and the
// fresh fragment of every reparsed graph source.
pub(super) fn fold(
    per_file: Vec<Option<ParsedFile>>,
) -> (HashMap<String, String>, HashMap<String, AnyFragment>) {
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
    (purposes, fresh_fragments)
}

// A TS/JS file whose AST pass yielded no purpose (zero-export or parse
// failure) is already the best this worker can produce this run --
// "ast-none" carries the same heuristic purpose text as a plain heuristic
// result, but (unlike "heuristic") `carries_best_source` above recognises it
// as best-obtainable for TS/JS, so an unchanged mtime is reused on the next
// map instead of being resent for reparse. C# is untouched -- it still gets
// "heuristic".
pub(super) fn fallback_source(rel: &str) -> &'static str {
    if parse::is_ts_js(rel) {
        "ast-none"
    } else {
        "heuristic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
