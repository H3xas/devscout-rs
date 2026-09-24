// The `impact` verb: its own numeric flags (`--hops`, `--iface-max-fanin`,
// `--hub-max-indegree`, `--pick`), a file-or-symbol seed that may be rewritten
// repo-relative, and a resolved-but-empty answer counted as a zero hit like
// every other verb's.

use std::path::Path;
use std::time::Instant;

use crate::graph;
use crate::query;
use crate::render;

use super::answer::{
    ambiguous_candidates_out, fallback_advised_out, finish_query, member_ambiguous_out,
    EXIT_NO_RESULT,
};
use super::args::{index_options, output_flags, parse_int_js, parse_pick};
use super::root::{repo_relative_arg, require_graph, require_repo_for_path};

const IMPACT_USAGE: &str = "usage: devscout impact <file|symbol> [--hops N] [--no-iface] [--no-guess] [--no-dispatch] [--no-bus] [--no-imports] [--iface-max-fanin N] [--hub-max-indegree N] [--pick N] [--json|--compact]";

/// The numeric/positional argument parse `cmd_impact` needs before it can
/// touch the repo or the graph: `--hops`/`--iface-max-fanin`/
/// `--hub-max-indegree`/`--pick`, and the query -- the first non-flag
/// argument that is not one of those four flags' own values.
struct ImpactArgs<'a> {
    hops: u32,
    iface_max_fanin: usize,
    hub_max_indegree: usize,
    pick: Option<usize>,
    query: &'a str,
}

fn parse_impact_args(args: &[String]) -> Result<ImpactArgs<'_>, (i32, String)> {
    let usage_err = || (2, IMPACT_USAGE.to_string());

    let mut hops: u32 = query::DEFAULT_HOPS;
    if let Some(idx) = args.iter().position(|a| a == "--hops") {
        let raw = args.get(idx + 1).map(String::as_str).unwrap_or("");
        match parse_int_js(raw) {
            Some(h) if h >= 1 => hops = h as u32,
            _ => return Err(usage_err()),
        }
    }

    // `--iface-max-fanin`'s own value is never the query. `0` is a legal value
    // and does not start with `--`, so without the guard below it would be picked
    // up as the seed. Parsed AFTER `--hops` and BEFORE the missing-query check.
    let mut iface_max_fanin: usize = query::DEFAULT_IFACE_MAX_FANIN;
    if let Some(idx) = args.iter().position(|a| a == "--iface-max-fanin") {
        let raw = args.get(idx + 1).map(String::as_str).unwrap_or("");
        match parse_int_js(raw) {
            Some(n) if n >= 0 => iface_max_fanin = n as usize,
            _ => return Err(usage_err()),
        }
    }

    // The same rule as `--iface-max-fanin`: its own value is never the query, and
    // `0` is a legal value that does not start with `--`.
    let mut hub_max_indegree: usize = query::DEFAULT_HUB_MAX_INDEGREE;
    if let Some(idx) = args.iter().position(|a| a == "--hub-max-indegree") {
        let raw = args.get(idx + 1).map(String::as_str).unwrap_or("");
        match parse_int_js(raw) {
            Some(n) if n >= 0 => hub_max_indegree = n as usize,
            _ => return Err(usage_err()),
        }
    }

    let Ok(pick) = parse_pick(args) else {
        return Err(usage_err());
    };

    // The first non-flag argument that is not the value of `--hops`,
    // `--iface-max-fanin`, `--hub-max-indegree`, or `--pick`.
    let mut query: Option<&str> = None;
    for (i, a) in args.iter().enumerate() {
        if a.starts_with("--") {
            continue;
        }
        if i > 0
            && (args[i - 1] == "--hops"
                || args[i - 1] == "--iface-max-fanin"
                || args[i - 1] == "--hub-max-indegree"
                || args[i - 1] == "--pick")
        {
            continue;
        }
        query = Some(a.as_str());
        break;
    }
    let Some(query) = query else {
        return Err(usage_err());
    };

    Ok(ImpactArgs {
        hops,
        iface_max_fanin,
        hub_max_indegree,
        pick,
        query,
    })
}

// `impact`. Check order: `--compact`+`--json` conflict, `--hops` parse (usage
// error on a bad/missing value), missing query, THEN `require_repo`, THEN the
// graph-present check.
pub(crate) fn cmd_impact(cwd: &Path, args: &[String]) -> (i32, String) {
    let start = Instant::now();
    let (json, compact) = match output_flags("impact", args) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let ImpactArgs {
        hops,
        iface_max_fanin,
        hub_max_indegree,
        pick,
        query: q,
    } = match parse_impact_args(args) {
        Ok(a) => a,
        Err(e) => return e,
    };

    let root = match require_repo_for_path(cwd, Some(q)) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };
    let q = repo_relative_arg(cwd, &root, q);
    let q = q.as_str();
    let g = match require_graph(&root) {
        Ok(g) => g,
        Err(e) => return e,
    };
    let index = query::load_graph_index_with(&g, &root, index_options(args));

    let build = |seed: &str| {
        query::build_impact_model(
            &index,
            seed,
            hops,
            query::DEFAULT_CAP,
            !args.iter().any(|a| a == "--no-iface"),
            iface_max_fanin,
            hub_max_indegree,
        )
    };
    // Same `--pick` narrowing rule `cmd_refs` applies: only a `MemberAmbiguous`
    // answer is ever re-resolved.
    let result = match build(q) {
        query::ImpactResult::MemberAmbiguous(candidates) => match pick {
            Some(n) if n <= candidates.len() => build(&query::qualified_seed(&candidates[n - 1])),
            Some(_) => return (2, IMPACT_USAGE.to_string()),
            None => query::ImpactResult::MemberAmbiguous(candidates),
        },
        other => other,
    };

    let answer = match result {
        query::ImpactResult::NotFound { kind } => {
            let (code, out) = fallback_advised_out(
                q,
                json,
                format!("no {} match for \"{q}\"", render::seed_kind_str(kind)),
            );
            (code, out, query::Outcome::FallbackAdvised, 0)
        }
        query::ImpactResult::Ambiguous { ids, .. } => {
            let count = ids.len();
            let (code, out) = ambiguous_candidates_out(&index, q, &ids, json);
            (code, out, query::Outcome::Ambiguous, count)
        }
        query::ImpactResult::MemberAmbiguous(candidates) => {
            let count = candidates.len();
            let (code, out) = member_ambiguous_out(q, &candidates, json);
            (code, out, query::Outcome::Ambiguous, count)
        }
        query::ImpactResult::Resolved(model) => {
            // `--no-imports` answers exactly as if the artifact were absent --
            // it is never even read, so this is the same "load nothing" path a
            // repo with no import configured always takes.
            let imported = if args.iter().any(|a| a == "--no-imports") {
                None
            } else {
                graph::read_imported_edges(&root).map(|edges| {
                    (
                        query::build_imported_section(&model, &edges, query::DEFAULT_CAP),
                        edges.provenance,
                    )
                })
            };
            let out = match (&imported, json, compact) {
                (Some((section, prov)), true, _) => {
                    query::json::impact_model_to_json_with_imports(q, &model, section, prov)
                }
                (Some((section, prov)), false, true) => {
                    render::render_impact_compact_with_imports(q, &model, section, prov)
                }
                (Some((section, prov)), false, false) => {
                    render::render_impact_text_with_imports(q, &model, section, prov)
                }
                (None, true, _) => query::json::impact_model_to_json(q, &model),
                (None, false, true) => render::render_impact_compact(q, &model),
                (None, false, false) => render::render_impact_text(q, &model),
            };
            // A resolved seed that reaches nothing beyond its own files, and
            // whose import (if any) named nothing foreign either, is the same
            // answer as an unresolved one -- empty -- and gets the same signal.
            let imported_count = imported.as_ref().map_or(0, |(s, _)| s.rows.len());
            let count = model.rows.len() + imported_count;
            if count == 0 {
                (EXIT_NO_RESULT, out, query::Outcome::ZeroHit, 0)
            } else {
                (0, out, query::Outcome::Hit, count)
            }
        }
    };
    finish_query(&root, "impact", q, start, json, answer)
}
