// The `impact` verb: its own numeric flags (`--hops`, `--iface-max-fanin`,
// `--hub-max-indegree`, `--pick`), a file-or-symbol seed that may be rewritten
// repo-relative, and a resolved-but-empty answer counted as a zero hit like
// every other verb's.

use std::path::Path;
use std::time::Instant;

use crate::exit::{EXIT_FAILURE, EXIT_NO_RESULT, EXIT_OK, EXIT_USAGE};
use crate::graph;
use crate::query;
use crate::render;
use crate::usage::IMPACT_USAGE;

use super::answer::{
    ambiguous_candidates_out, begin_validation, fallback_advised_out, finish_query,
    member_ambiguous_out, with_next_trailer,
};
use super::args::{index_options, output_flags, parse_int_js, parse_pick, Pick};
use super::root::{repo_relative_arg, require_graph, require_repo_for_path};
use super::sections::{self, SectionAnswer};

/// The output flags the resolved answer reads, gathered so the answer builder
/// takes one value rather than three positional booleans. `Copy` because the
/// per-candidate builder behind a `--pick` narrowing reads the same flags for
/// every section.
#[derive(Clone, Copy)]
struct ImpactFlags {
    json: bool,
    compact: bool,
    no_imports: bool,
    no_iface: bool,
}

/// The numeric/positional argument parse `cmd_impact` needs before it can
/// touch the repo or the graph: `--hops`/`--iface-max-fanin`/
/// `--hub-max-indegree`/`--pick`, and the query -- the first non-flag
/// argument that is not one of those four flags' own values.
struct ImpactArgs<'a> {
    hops: u32,
    iface_max_fanin: usize,
    hub_max_indegree: usize,
    pick: Option<Pick>,
    query: &'a str,
}

fn parse_impact_args(args: &[String]) -> Result<ImpactArgs<'_>, (i32, String)> {
    let usage_err = || (EXIT_USAGE, IMPACT_USAGE.to_string());

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

    let flags = ImpactFlags {
        json,
        compact,
        no_imports: args.iter().any(|a| a == "--no-imports"),
        no_iface: args.iter().any(|a| a == "--no-iface"),
    };
    let root = match require_repo_for_path(cwd, Some(q)) {
        Ok(r) => r,
        Err(e) => return (EXIT_FAILURE, format!("error: {e}")),
    };
    // One validation per invocation, opened before any index artifact is read.
    begin_validation(&root, cwd);
    let q = repo_relative_arg(cwd, &root, q);
    let q = q.as_str();
    let g = match require_graph(&root) {
        Ok(g) => g,
        Err(e) => return e,
    };
    let index = query::load_graph_index_with(&g, &root, index_options(args));

    let holds_iface = !args.iter().any(|a| a == "--no-iface");
    // Same `--pick` narrowing rule `cmd_refs` applies: a `--pick` answer
    // narrows one candidate of an ambiguous seed, and `--pick all` answers
    // each in its own section. A type candidate starts from its def id rather
    // than a seed spelling, because `looks_like_file_path` reads a dotted type
    // id as a file path.
    let build = query::build_impact_model(
        &index,
        q,
        hops,
        query::DEFAULT_CAP,
        holds_iface,
        iface_max_fanin,
        hub_max_indegree,
    );
    let answer = match (pick, build) {
        (Some(pick), query::ImpactResult::MemberAmbiguous(candidates)) => {
            let picked = sections::narrow(
                sections::Candidates::from_members(q, &candidates),
                pick,
                json,
                IMPACT_USAGE,
                |picked| {
                    impact_candidate_out(
                        &index,
                        &root,
                        q,
                        picked,
                        flags,
                        hops,
                        iface_max_fanin,
                        hub_max_indegree,
                    )
                },
            );
            match picked {
                Ok(a) => a,
                Err(e) => return e,
            }
        }
        (Some(pick), query::ImpactResult::Ambiguous { ids, .. }) => {
            let picked = sections::narrow(
                sections::Candidates::from_types(q, &index, &ids),
                pick,
                json,
                IMPACT_USAGE,
                |picked| {
                    impact_candidate_out(
                        &index,
                        &root,
                        q,
                        picked,
                        flags,
                        hops,
                        iface_max_fanin,
                        hub_max_indegree,
                    )
                },
            );
            match picked {
                Ok(a) => a,
                Err(e) => return e,
            }
        }
        (_, result) => impact_answer(&index, &root, q, result, flags),
    };
    finish_query(&root, "impact", q, start, json, answer.into_tuple())
}

/// One `--pick` candidate's `impact` answer.
///
/// Every candidate is walked as a single resolved SYMBOL seed, and the
/// rendered answer echoes the seed the caller typed -- the same echo the
/// query layer's own unique-type path produces, so a section and a `--pick
/// <n>` answer are the same bytes.
#[allow(clippy::too_many_arguments)]
fn impact_candidate_out(
    index: &query::GraphIndex,
    root: &Path,
    q: &str,
    picked: &query::PickedSeed,
    flags: ImpactFlags,
    hops: u32,
    iface_max_fanin: usize,
    hub_max_indegree: usize,
) -> SectionAnswer {
    let model = query::pick_impact(
        index,
        picked,
        hops,
        query::DEFAULT_CAP,
        !flags.no_iface,
        iface_max_fanin,
        hub_max_indegree,
    );
    impact_answer(index, root, q, model, flags)
}

/// The rendering AND telemetry facts for an `impact` answer.
fn impact_answer(
    index: &query::GraphIndex,
    root: &Path,
    q: &str,
    result: query::ImpactResult,
    flags: ImpactFlags,
) -> SectionAnswer {
    match result {
        query::ImpactResult::NotFound { kind } => {
            let (code, out) = fallback_advised_out(
                q,
                flags.json,
                format!("no {} match for \"{q}\"", render::seed_kind_str(kind)),
            );
            SectionAnswer::new(code, out, query::Outcome::FallbackAdvised, 0, Vec::new())
        }
        query::ImpactResult::Ambiguous { ids, .. } => {
            let count = ids.len();
            let (code, out) = ambiguous_candidates_out(index, q, &ids, flags.json);
            SectionAnswer::new(code, out, query::Outcome::Ambiguous, count, Vec::new())
        }
        query::ImpactResult::MemberAmbiguous(candidates) => {
            let count = candidates.len();
            let (code, out) = member_ambiguous_out(q, &candidates, flags.json);
            SectionAnswer::new(code, out, query::Outcome::Ambiguous, count, Vec::new())
        }
        query::ImpactResult::Resolved(model) => resolved_out(index, root, q, &model, flags),
    }
}

/// The resolved `impact` answer: the imported section when an import is
/// configured, the rendered model, the exit code the two together earn, and
/// the model's own suggestions.
fn resolved_out(
    index: &query::GraphIndex,
    root: &Path,
    q: &str,
    model: &query::ImpactModel,
    flags: ImpactFlags,
) -> SectionAnswer {
    // `--no-imports` answers exactly as if the artifact were absent -- it is
    // never even read, so this is the same "load nothing" path a repo with no
    // import configured always takes.
    let imported = if flags.no_imports {
        None
    } else {
        graph::read_imported_edges(root).map(|edges| {
            (
                query::build_imported_section(model, &edges, query::DEFAULT_CAP),
                edges.provenance,
            )
        })
    };
    let next = query::impact_next(index, model);
    let out = match (&imported, flags.json, flags.compact) {
        (Some((section, prov)), true, _) => {
            query::json::impact_model_to_json_with_imports_and_next(q, model, section, prov, &next)
        }
        (Some((section, prov)), false, true) => with_next_trailer(
            render::render_impact_compact_with_imports(q, model, section, prov),
            &next,
        ),
        (Some((section, prov)), false, false) => with_next_trailer(
            render::render_impact_text_with_imports(q, model, section, prov),
            &next,
        ),
        (None, true, _) => query::json::impact_model_to_json_with_next(q, model, &next),
        (None, false, true) => with_next_trailer(render::render_impact_compact(q, model), &next),
        (None, false, false) => with_next_trailer(render::render_impact_text(q, model), &next),
    };
    // A resolved seed that reaches nothing beyond its own files, and whose
    // import (if any) named nothing foreign either, is the same answer as an
    // unresolved one -- empty -- and gets the same signal.
    let imported_count = imported.as_ref().map_or(0, |(s, _)| s.rows.len());
    let count = model.rows.len() + imported_count;
    if count == 0 {
        SectionAnswer::new(EXIT_NO_RESULT, out, query::Outcome::ZeroHit, 0, next)
    } else {
        SectionAnswer::new(EXIT_OK, out, query::Outcome::Hit, count, next)
    }
}
