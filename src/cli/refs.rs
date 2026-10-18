// The `refs` verb: usage text, argument parsing, and the outcome-to-answer
// mapping every one of its result shapes renders through. `member_models_out`
// and `refs_model_row_count` are shared with `read`, whose own bare-member
// answer is refs' own, unchanged.

use std::path::Path;
use std::time::Instant;

use crate::exit::{EXIT_FAILURE, EXIT_NO_RESULT, EXIT_OK, EXIT_USAGE};
use crate::query;
use crate::render;
use crate::usage::REFS_USAGE;

use super::answer::{
    ambiguous_candidates_out, begin_validation, fallback_advised_out, finish_query,
    member_ambiguous_out, with_next_trailer, ZERO_HIT_REFS,
};
use super::args::{first_positional, index_options, output_flags, parse_pick};
use super::root::{require_graph, require_repo};
use super::sections::{self, SectionAnswer};

// The row count `refs`/`read` telemetry reports for a resolved answer: every
// inbound row plus, under `--out`, every outbound row -- the same rows the
// text/JSON renderers already walk, counted here once rather than re-parsed
// out of the rendered answer. The TS tables count too, so a TypeScript answer
// never reports zero rows beside a non-empty table.
pub(crate) fn refs_model_row_count(model: &query::RefsModel) -> usize {
    let mut n = model.inbound.inherits.rows.len()
        + model.inbound.uses_type.rows.len()
        + model.inbound.uses_member.rows.len()
        + ts_row_count(&model.inbound.ts);
    if let Some(ob) = &model.outbound {
        n += ob.inherits.rows.len()
            + ob.uses_type.rows.len()
            + ob.uses_member.rows.len()
            + ob.imports.rows.len()
            + ts_row_count(&ob.ts);
    }
    n
}

fn ts_row_count<R>(tables: &query::TsTables<R>) -> usize {
    tables.named().iter().map(|(_, t)| t.rows.len()).sum()
}

// The rows of a member answer whose owner is a TypeScript class come from
// Angular templates alone, because the index reads no TypeScript member
// access. The answer says so, so that zero rows never reads as "unused".
const TEMPLATE_READS_NOTE: &str =
    "note: rows are Angular template reads only. TypeScript member reads are not indexed.";

fn template_reads_only(index: &query::GraphIndex, model: &query::RefsModel) -> bool {
    model
        .id
        .rsplit_once('.')
        .and_then(|(owner, _)| index.by_id.get(owner))
        .is_some_and(|&i| crate::parse::ts_grammar_for(&index.graph.defs[i].file).is_some())
}

// A bare member answers with one ordinary refs model per declaring type, so
// each block renders through the very renderer a type uses and `--json` wraps
// those same objects in an array rather than reshaping them. A member the
// graph declares but no verified edge reaches is that same answer with its
// tables empty: the seed resolved, so this is `refs`' own zero hit and not the
// unresolved case `fallback_advised_out` renders.
pub(crate) fn member_models_out(
    index: &query::GraphIndex,
    q: &str,
    json: bool,
    compact: bool,
    models: &[query::RefsModel],
) -> SectionAnswer {
    let (code, outcome) = if models.iter().all(|m| m.inbound.uses_member.total == 0) {
        (EXIT_NO_RESULT, query::Outcome::ZeroHit)
    } else {
        (EXIT_OK, query::Outcome::Hit)
    };
    // Each member answers for itself; the union is what the answer as a whole
    // suggests, and it is what the text trailer states once, after every block.
    let member_next: Vec<Vec<query::NextArgv>> =
        models.iter().map(|m| query::refs_next(index, m)).collect();
    let union = query::merge_suggestions(member_next.clone());
    let template_only: Vec<bool> = models
        .iter()
        .map(|m| template_reads_only(index, m))
        .collect();
    let out = if json {
        query::json::member_refs_to_json_with_next(
            q,
            models,
            outcome,
            &member_next,
            &union,
            &template_only,
        )
    } else {
        let render: fn(&query::RefsModel) -> String = if compact {
            render::render_refs_compact
        } else {
            render::render_refs_text
        };
        let mut body = models.iter().map(render).collect::<Vec<_>>().join("\n");
        if template_only.contains(&true) {
            body = format!("{}\n{TEMPLATE_READS_NOTE}", body.trim_end_matches('\n'));
        }
        with_next_trailer(body, &union)
    };
    let count = models.iter().map(refs_model_row_count).sum();
    SectionAnswer::new(code, out, outcome, count, union)
}

// `refs`. Check order: `--compact`+`--json` conflict, then missing query, THEN
// `require_repo`, THEN the graph-present check -- a query run with no repo
// present reports the missing-repo error even if the query itself is also
// absent-adjacent.
pub(crate) fn cmd_refs(cwd: &Path, args: &[String]) -> (i32, String, Option<&'static str>) {
    let start = Instant::now();
    let (json, compact) = match output_flags("refs", args) {
        Ok(v) => v,
        Err((code, out)) => return (code, out, None),
    };
    let Ok(pick) = parse_pick(args) else {
        return (EXIT_USAGE, REFS_USAGE.to_string(), None);
    };
    let Some(q) = first_positional(args) else {
        return (EXIT_USAGE, REFS_USAGE.to_string(), None);
    };
    let out = args.iter().any(|a| a == "--out");
    // `--all` lifts only `query::OUTBOUND_CAP`; it is otherwise inert without
    // `--out`, same as `--out` is inert on the bare-member fallback path.
    let all_out = args.iter().any(|a| a == "--all");
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (EXIT_FAILURE, format!("error: {e}"), None),
    };
    // One validation per invocation, opened before any index artifact is read.
    begin_validation(&root, cwd);
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
    // A `--pick` answer narrows ONE candidate of an ambiguous seed: a member
    // name several types declare, or a type name several defs carry. Both
    // `--pick <n>` and a `--pick all` section go through the same per-candidate
    // builder below, so the two cannot disagree about a candidate. Without
    // `--pick`, an ambiguous seed still refuses and lists its candidates.
    let answer = match (pick, build(q)) {
        (Some(pick), query::RefsResult::MemberAmbiguous(candidates)) => {
            let picked = sections::narrow(
                sections::Candidates::from_members(q, &candidates),
                pick,
                json,
                REFS_USAGE,
                |picked| refs_candidate_out(&index, q, json, compact, picked, out, all_out),
            );
            match picked {
                Ok(a) => a,
                Err(e) => return (e.0, e.1, None),
            }
        }
        (Some(pick), query::RefsResult::Ambiguous(ids)) => {
            let picked = sections::narrow(
                sections::Candidates::from_types(q, &index, &ids),
                pick,
                json,
                REFS_USAGE,
                |picked| refs_candidate_out(&index, q, json, compact, picked, out, all_out),
            );
            match picked {
                Ok(a) => a,
                Err(e) => return (e.0, e.1, None),
            }
        }
        (_, result) => refs_answer(&index, q, json, compact, result),
    };

    let note = (answer.outcome != query::Outcome::ZeroHit).then_some(ZERO_HIT_REFS);
    let (code, out) = finish_query(&root, "refs", q, start, json, answer.into_tuple());
    (code, out, note)
}

// One `--pick` candidate's answer: the candidate's own seed run through the
// same model builder the typed seed uses.
fn refs_candidate_out(
    index: &query::GraphIndex,
    q: &str,
    json: bool,
    compact: bool,
    picked: &query::PickedSeed,
    out: bool,
    all_out: bool,
) -> SectionAnswer {
    refs_answer(
        index,
        q,
        json,
        compact,
        query::pick_refs(
            index,
            picked,
            out,
            query::DEFAULT_CAP,
            query::INBOUND_CAP,
            query::OUTBOUND_CAP,
            all_out,
        ),
    )
}

// The rendering AND telemetry facts (`Outcome`, row count, suggestions) for a
// `refs` answer, derived together at the single point the answer is rendered
// so the telemetry line can never disagree with the bytes that went to stdout.
fn refs_answer(
    index: &query::GraphIndex,
    q: &str,
    json: bool,
    compact: bool,
    result: query::RefsResult,
) -> SectionAnswer {
    match result {
        query::RefsResult::NotFound => {
            let (code, out) = fallback_advised_out(q, json, format!("no symbol matches \"{q}\""));
            SectionAnswer::new(code, out, query::Outcome::FallbackAdvised, 0, Vec::new())
        }
        query::RefsResult::Ambiguous(ids) => {
            let count = ids.len();
            let (code, out) = ambiguous_candidates_out(index, q, &ids, json);
            SectionAnswer::new(code, out, query::Outcome::Ambiguous, count, Vec::new())
        }
        query::RefsResult::MemberAmbiguous(candidates) => {
            let count = candidates.len();
            let (code, out) = member_ambiguous_out(q, &candidates, json);
            SectionAnswer::new(code, out, query::Outcome::Ambiguous, count, Vec::new())
        }
        query::RefsResult::Members(models) => member_models_out(index, q, json, compact, &models),
        query::RefsResult::Resolved(model) => {
            let count = refs_model_row_count(&model);
            let next = query::refs_next(index, &model);
            let out = if json {
                query::json::refs_model_to_json_with_next(&model, &next)
            } else if compact {
                with_next_trailer(render::render_refs_compact(&model), &next)
            } else {
                with_next_trailer(render::render_refs_text(&model), &next)
            };
            SectionAnswer::new(EXIT_OK, out, query::Outcome::Hit, count, next)
        }
    }
}
