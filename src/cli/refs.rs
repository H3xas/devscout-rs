// The `refs` verb: usage text, argument parsing, and the outcome-to-answer
// mapping every one of its result shapes renders through. `member_models_out`
// and `refs_model_row_count` are shared with `read`, whose own bare-member
// answer is refs' own, unchanged.

use std::path::Path;
use std::time::Instant;

use crate::query;
use crate::render;

use super::answer::{
    ambiguous_candidates_out, fallback_advised_out, finish_query, member_ambiguous_out,
    EXIT_NO_RESULT, ZERO_HIT_REFS,
};
use super::args::{first_positional, index_options, output_flags, parse_pick};
use super::root::{require_graph, require_repo};

// `refs`. Check order: `--compact`+`--json` conflict, then missing query, THEN
// `require_repo`, THEN the graph-present check -- a query run with no repo
// present reports the missing-repo error even if the query itself is also
// absent-adjacent.
const REFS_USAGE: &str =
    "usage: devscout refs <symbol> [--out] [--all] [--no-guess] [--no-dispatch] [--pick N] [--json|--compact]";

// The row count `refs`/`read` telemetry reports for a resolved answer: every
// inbound row plus, under `--out`, every outbound row -- the same rows the
// text/JSON renderers already walk, counted here once rather than re-parsed
// out of the rendered answer.
pub(crate) fn refs_model_row_count(model: &query::RefsModel) -> usize {
    let mut n = model.inbound.inherits.rows.len()
        + model.inbound.uses_type.rows.len()
        + model.inbound.uses_member.rows.len();
    if let Some(ob) = &model.outbound {
        n += ob.inherits.rows.len()
            + ob.uses_type.rows.len()
            + ob.uses_member.rows.len()
            + ob.imports.rows.len();
    }
    n
}

// A bare member answers with one ordinary refs model per declaring type, so
// each block renders through the very renderer a type uses and `--json` wraps
// those same objects in an array rather than reshaping them. A member the
// graph declares but no verified edge reaches is that same answer with its
// tables empty: the seed resolved, so this is `refs`' own zero hit and not the
// unresolved case `fallback_advised_out` renders.
pub(crate) fn member_models_out(
    q: &str,
    json: bool,
    compact: bool,
    models: &[query::RefsModel],
) -> (i32, String, query::Outcome, usize) {
    let (code, outcome) = if models.iter().all(|m| m.inbound.uses_member.total == 0) {
        (EXIT_NO_RESULT, query::Outcome::ZeroHit)
    } else {
        (0, query::Outcome::Hit)
    };
    let out = if json {
        query::json::member_refs_to_json(q, models, outcome)
    } else {
        let render: fn(&query::RefsModel) -> String = if compact {
            render::render_refs_compact
        } else {
            render::render_refs_text
        };
        models.iter().map(render).collect::<Vec<_>>().join("\n")
    };
    let count = models.iter().map(refs_model_row_count).sum();
    (code, out, outcome, count)
}

pub(crate) fn cmd_refs(cwd: &Path, args: &[String]) -> (i32, String, Option<&'static str>) {
    let start = Instant::now();
    let (json, compact) = match output_flags("refs", args) {
        Ok(v) => v,
        Err((code, out)) => return (code, out, None),
    };
    let Ok(pick) = parse_pick(args) else {
        return (2, REFS_USAGE.to_string(), None);
    };
    let Some(q) = first_positional(args) else {
        return (2, REFS_USAGE.to_string(), None);
    };
    let out = args.iter().any(|a| a == "--out");
    // `--all` lifts only `query::OUTBOUND_CAP`; it is otherwise inert without
    // `--out`, same as `--out` is inert on the bare-member fallback path.
    let all_out = args.iter().any(|a| a == "--all");
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}"), None),
    };
    let g = match require_graph(&root) {
        Ok(g) => g,
        Err((code, out)) => return (code, out, None),
    };
    let index = query::load_graph_index_with(&g, &root, index_options(args));

    let build = |seed: &str| {
        query::build_refs_model(
            &index,
            seed,
            out,
            query::DEFAULT_CAP,
            query::INBOUND_CAP,
            query::OUTBOUND_CAP,
            all_out,
        )
    };
    // `--pick n` only ever narrows a `MemberAmbiguous` answer: it re-resolves
    // against the SAME synthetic `Owner.Member` seed a caller could have typed
    // by hand, rather than adding a second resolution path only `--pick`
    // takes. An out-of-range `n` is a usage error; `--pick` is silently
    // inert on every other outcome.
    let result = match build(q) {
        query::RefsResult::MemberAmbiguous(candidates) => match pick {
            Some(n) if n <= candidates.len() => build(&query::qualified_seed(&candidates[n - 1])),
            Some(_) => return (2, REFS_USAGE.to_string(), None),
            None => query::RefsResult::MemberAmbiguous(candidates),
        },
        other => other,
    };

    let answer = refs_result_out(&index, q, json, compact, result);
    let note = (answer.2 != query::Outcome::ZeroHit).then_some(ZERO_HIT_REFS);
    let (code, out) = finish_query(&root, "refs", q, start, answer);
    (code, out, note)
}

// The rendering AND telemetry facts (`Outcome`, row count) for a `refs`
// answer, derived together at the single point the answer is rendered so the
// telemetry line can never disagree with the bytes that went to stdout.
fn refs_result_out(
    index: &query::GraphIndex,
    q: &str,
    json: bool,
    compact: bool,
    result: query::RefsResult,
) -> (i32, String, query::Outcome, usize) {
    match result {
        query::RefsResult::NotFound => {
            let (code, out) = fallback_advised_out(q, json, format!("no symbol matches \"{q}\""));
            (code, out, query::Outcome::FallbackAdvised, 0)
        }
        query::RefsResult::Ambiguous(ids) => {
            let count = ids.len();
            let (code, out) = ambiguous_candidates_out(index, q, &ids, json);
            (code, out, query::Outcome::Ambiguous, count)
        }
        query::RefsResult::MemberAmbiguous(candidates) => {
            let count = candidates.len();
            let (code, out) = member_ambiguous_out(q, &candidates, json);
            (code, out, query::Outcome::Ambiguous, count)
        }
        query::RefsResult::Members(models) => member_models_out(q, json, compact, &models),
        query::RefsResult::Resolved(model) => {
            let count = refs_model_row_count(&model);
            let out = if json {
                query::json::refs_model_to_json(&model)
            } else if compact {
                render::render_refs_compact(&model)
            } else {
                render::render_refs_text(&model)
            };
            (0, out, query::Outcome::Hit, count)
        }
    }
}
