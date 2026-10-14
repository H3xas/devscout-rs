// The `tests` verb -- tests reaching a symbol. Named `coverage` rather than
// `tests` to avoid colliding with this module's own `#[cfg(test)] mod tests`
// sibling.

use std::path::Path;
use std::time::Instant;

use crate::query;
use crate::render;

use super::answer::{
    ambiguous_candidates_out, begin_validation, fallback_advised_out, finish_query,
    member_ambiguous_out, with_next_trailer,
};
use super::args::{first_positional, index_options, output_flags, parse_pick};
use super::root::{require_graph, require_repo};

const TESTS_USAGE: &str =
    "usage: devscout tests <symbol> [--no-guess] [--no-dispatch] [--no-bus] [--pick N] [--json|--compact]";

// `tests`. Mirrors `cmd_refs` -- same flag conflict, same missing-query usage
// error, same `require_repo`/graph-present order, same notfound/ambiguous exits.
pub(crate) fn cmd_tests(cwd: &Path, args: &[String]) -> (i32, String) {
    let start = Instant::now();
    let (json, compact) = match output_flags("tests", args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Ok(pick) = parse_pick(args) else {
        return (2, TESTS_USAGE.to_string());
    };
    let Some(q) = first_positional(args) else {
        return (2, TESTS_USAGE.to_string());
    };
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}")),
    };
    // One validation per invocation, opened before any index artifact is read.
    begin_validation(&root, cwd);
    let g = match require_graph(&root) {
        Ok(g) => g,
        Err(e) => return e,
    };
    let index = query::load_graph_index_with(&g, &root, index_options(args));

    // Same `--pick` narrowing rule `cmd_refs` applies: only a `MemberAmbiguous`
    // answer is ever re-resolved.
    let result = match query::build_tests_model(&index, q) {
        query::TestsResult::MemberAmbiguous(candidates) => match pick {
            Some(n) if n <= candidates.len() => {
                query::build_tests_model(&index, &query::qualified_seed(&candidates[n - 1]))
            }
            Some(_) => return (2, TESTS_USAGE.to_string()),
            None => query::TestsResult::MemberAmbiguous(candidates),
        },
        other => other,
    };

    let answer = match result {
        query::TestsResult::NotFound => {
            let (code, out) = fallback_advised_out(q, json, format!("no symbol matches \"{q}\""));
            (code, out, query::Outcome::FallbackAdvised, 0)
        }
        query::TestsResult::Ambiguous(ids) => {
            let count = ids.len();
            let (code, out) = ambiguous_candidates_out(&index, q, &ids, json);
            (code, out, query::Outcome::Ambiguous, count)
        }
        query::TestsResult::MemberAmbiguous(candidates) => {
            let count = candidates.len();
            let (code, out) = member_ambiguous_out(q, &candidates, json);
            (code, out, query::Outcome::Ambiguous, count)
        }
        query::TestsResult::Resolved(model) => {
            let count = model.rows.len();
            let next = query::tests_next(&index, &model);
            let out = if json {
                query::json::tests_model_to_json_with_next(&model, &next)
            } else if compact {
                with_next_trailer(render::render_tests_compact(&model), &next)
            } else {
                with_next_trailer(render::render_tests_text(&model), &next)
            };
            (0, out, query::Outcome::Hit, count)
        }
    };
    finish_query(&root, "tests", q, start, json, answer)
}
