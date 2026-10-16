// The `tests` verb -- tests reaching a symbol. Named `coverage` rather than
// `tests` to avoid colliding with this module's own `#[cfg(test)] mod tests`
// sibling.

use std::path::Path;
use std::time::Instant;

use crate::exit::{EXIT_FAILURE, EXIT_OK, EXIT_USAGE};
use crate::query;
use crate::render;
use crate::usage::TESTS_USAGE;

use super::answer::{
    ambiguous_candidates_out, begin_validation, fallback_advised_out, finish_query,
    member_ambiguous_out, with_next_trailer,
};
use super::args::{first_positional, index_options, output_flags, parse_pick};
use super::root::{require_graph, require_repo};
use super::sections::{self, SectionAnswer};

// `tests`. Mirrors `cmd_refs` -- same flag conflict, same missing-query usage
// error, same `require_repo`/graph-present order, same notfound/ambiguous exits.
pub(crate) fn cmd_tests(cwd: &Path, args: &[String]) -> (i32, String) {
    let start = Instant::now();
    let (json, compact) = match output_flags("tests", args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Ok(pick) = parse_pick(args) else {
        return (EXIT_USAGE, TESTS_USAGE.to_string());
    };
    let Some(q) = first_positional(args) else {
        return (EXIT_USAGE, TESTS_USAGE.to_string());
    };
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (EXIT_FAILURE, format!("error: {e}")),
    };
    // One validation per invocation, opened before any index artifact is read.
    begin_validation(&root, cwd);
    let g = match require_graph(&root) {
        Ok(g) => g,
        Err(e) => return e,
    };
    let index = query::load_graph_index_with(&g, &root, index_options(args));

    // Same `--pick` narrowing rule `cmd_refs` applies: a `--pick` answer
    // narrows one candidate of an ambiguous seed, and `--pick all` answers
    // each in its own section.
    let answer = match (pick, query::build_tests_model(&index, q)) {
        (Some(pick), query::TestsResult::MemberAmbiguous(candidates)) => {
            let picked = sections::narrow(
                sections::Candidates::from_members(q, &candidates),
                pick,
                json,
                TESTS_USAGE,
                |picked| tests_candidate_out(&index, q, json, compact, picked),
            );
            match picked {
                Ok(a) => a,
                Err(e) => return e,
            }
        }
        (Some(pick), query::TestsResult::Ambiguous(ids)) => {
            let picked = sections::narrow(
                sections::Candidates::from_types(q, &index, &ids),
                pick,
                json,
                TESTS_USAGE,
                |picked| tests_candidate_out(&index, q, json, compact, picked),
            );
            match picked {
                Ok(a) => a,
                Err(e) => return e,
            }
        }
        (_, result) => tests_answer(&index, q, json, compact, result),
    };
    finish_query(&root, "tests", q, start, json, answer.into_tuple())
}

// One `--pick` candidate's `tests` answer: the candidate's own result, which
// echoes the seed the caller typed for a member candidate and the def id for a
// type candidate, rendered by the same answer function a typed seed uses.
fn tests_candidate_out(
    index: &query::GraphIndex,
    q: &str,
    json: bool,
    compact: bool,
    picked: &query::PickedSeed,
) -> SectionAnswer {
    tests_answer(index, q, json, compact, query::pick_tests(index, picked))
}

// The rendering AND telemetry facts for a `tests` answer.
fn tests_answer(
    index: &query::GraphIndex,
    q: &str,
    json: bool,
    compact: bool,
    result: query::TestsResult,
) -> SectionAnswer {
    match result {
        query::TestsResult::NotFound => {
            let (code, out) = fallback_advised_out(q, json, format!("no symbol matches \"{q}\""));
            SectionAnswer::new(code, out, query::Outcome::FallbackAdvised, 0, Vec::new())
        }
        query::TestsResult::Ambiguous(ids) => {
            let count = ids.len();
            let (code, out) = ambiguous_candidates_out(index, q, &ids, json);
            SectionAnswer::new(code, out, query::Outcome::Ambiguous, count, Vec::new())
        }
        query::TestsResult::MemberAmbiguous(candidates) => {
            let count = candidates.len();
            let (code, out) = member_ambiguous_out(q, &candidates, json);
            SectionAnswer::new(code, out, query::Outcome::Ambiguous, count, Vec::new())
        }
        query::TestsResult::Resolved(model) => {
            let count = model.rows.len();
            let next = query::tests_next(index, &model);
            let out = if json {
                query::json::tests_model_to_json_with_next(&model, &next)
            } else if compact {
                with_next_trailer(render::render_tests_compact(&model), &next)
            } else {
                with_next_trailer(render::render_tests_text(&model), &next)
            };
            SectionAnswer::new(EXIT_OK, out, query::Outcome::Hit, count, next)
        }
    }
}
