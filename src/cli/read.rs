// The `read` verb: a declaration span layered on `refs`' own resolution.
// Every non-resolved outcome (not-found, ambiguous, member-ambiguous, the
// bare-member answer) reuses `refs`' own renderer unchanged.

use std::path::Path;
use std::time::Instant;

use crate::exit::{EXIT_FAILURE, EXIT_OK, EXIT_USAGE};
use crate::query;
use crate::render;
use crate::usage::READ_USAGE;

use super::answer::{
    ambiguous_candidates_out, begin_validation, fallback_advised_out, finish_query,
    member_ambiguous_out, with_next_trailer, ZERO_HIT_READ,
};
use super::args::{first_positional, index_options, output_flags, parse_pick};
use super::refs::{member_models_out, refs_model_row_count};
use super::root::{require_graph, require_repo};
use super::sections::{self, SectionAnswer};

// `read`. Check order is `refs`' own: flag conflict, missing query,
// `require_repo`, graph-present. The resolution IS refs' --
// `build_read_model` wraps `build_refs_model` and changes nothing about how
// a name becomes an answer -- so the ambiguity and zero-hit discipline
// cannot drift between the two verbs; only the resolved arm grows the
// declaration span.

pub(crate) fn cmd_read(cwd: &Path, args: &[String]) -> (i32, String, Option<&'static str>) {
    let start = Instant::now();
    let (json, compact) = match output_flags("read", args) {
        Ok(v) => v,
        Err((code, out)) => return (code, out, None),
    };
    let Ok(pick) = parse_pick(args) else {
        return (EXIT_USAGE, READ_USAGE.to_string(), None);
    };
    let Some(q) = first_positional(args) else {
        return (EXIT_USAGE, READ_USAGE.to_string(), None);
    };
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

    // Same `--pick` narrowing rule `cmd_refs` applies: a `--pick` answer
    // narrows one candidate of an ambiguous seed, and `--pick all` answers
    // each in its own section. `read`'s per-candidate builder is `refs`' own,
    // so the two verbs cannot drift apart on a candidate.
    let answer = match (pick, query::build_read_model(&index, q)) {
        (Some(pick), query::ReadResult::MemberAmbiguous(candidates)) => {
            let picked = sections::narrow(
                sections::Candidates::from_members(q, &candidates),
                pick,
                json,
                READ_USAGE,
                |picked| read_candidate_out(&index, q, json, compact, picked),
            );
            match picked {
                Ok(a) => a,
                Err(e) => return (e.0, e.1, None),
            }
        }
        (Some(pick), query::ReadResult::Ambiguous(ids)) => {
            let picked = sections::narrow(
                sections::Candidates::from_types(q, &index, &ids),
                pick,
                json,
                READ_USAGE,
                |picked| read_candidate_out(&index, q, json, compact, picked),
            );
            match picked {
                Ok(a) => a,
                Err(e) => return (e.0, e.1, None),
            }
        }
        (_, result) => read_answer(&index, q, json, compact, result),
    };
    let note = (answer.outcome != query::Outcome::ZeroHit).then_some(ZERO_HIT_READ);
    let (code, out) = finish_query(&root, "read", q, start, json, answer.into_tuple());
    (code, out, note)
}

// One `--pick` candidate's `read` answer: the candidate's own result, rendered
// by the same answer function a typed seed uses. A member candidate's result
// is `refs`' own member answer -- a member carries no declaration-span fact to
// add -- which is exactly what a typed `Owner.Member` seed produced too.
fn read_candidate_out(
    index: &query::GraphIndex,
    q: &str,
    json: bool,
    compact: bool,
    picked: &query::PickedSeed,
) -> SectionAnswer {
    read_answer(index, q, json, compact, query::pick_read(index, picked))
}

// The rendering AND telemetry facts for a `read` answer.
fn read_answer(
    index: &query::GraphIndex,
    q: &str,
    json: bool,
    compact: bool,
    result: query::ReadResult,
) -> SectionAnswer {
    match result {
        query::ReadResult::NotFound => {
            let (code, out) = fallback_advised_out(q, json, format!("no symbol matches \"{q}\""));
            SectionAnswer::new(code, out, query::Outcome::FallbackAdvised, 0, Vec::new())
        }
        query::ReadResult::Ambiguous(ids) => {
            let count = ids.len();
            let (code, out) = ambiguous_candidates_out(index, q, &ids, json);
            SectionAnswer::new(code, out, query::Outcome::Ambiguous, count, Vec::new())
        }
        query::ReadResult::MemberAmbiguous(candidates) => {
            let count = candidates.len();
            let (code, out) = member_ambiguous_out(q, &candidates, json);
            SectionAnswer::new(code, out, query::Outcome::Ambiguous, count, Vec::new())
        }
        // `read`'s member answer IS refs' -- a member carries no
        // declaration-span fact to add -- so it takes the same exit code and
        // the same word, rather than a second shape to keep in step.
        query::ReadResult::Members(models) => member_models_out(index, q, json, compact, &models),
        query::ReadResult::Resolved(model) => {
            let count = refs_model_row_count(&model.refs);
            // `read` answers with refs' own inbound model, so it carries refs'
            // own suggestions rather than a second mapping to keep in step.
            let next = query::refs_next(index, &model.refs);
            let out = if json {
                query::json::read_model_to_json_with_next(&model, &next)
            } else if compact {
                with_next_trailer(render::render_read_compact(&model), &next)
            } else {
                with_next_trailer(render::render_read_text(&model), &next)
            };
            SectionAnswer::new(EXIT_OK, out, query::Outcome::Hit, count, next)
        }
    }
}
