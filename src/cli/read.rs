// The `read` verb: a declaration span layered on `refs`' own resolution.
// Every non-resolved outcome (not-found, ambiguous, member-ambiguous, the
// bare-member answer) reuses `refs`' own renderer unchanged.

use std::path::Path;
use std::time::Instant;

use crate::query;
use crate::render;

use super::answer::{
    ambiguous_candidates_out, fallback_advised_out, finish_query, member_ambiguous_out,
    ZERO_HIT_READ,
};
use super::args::{first_positional, index_options, output_flags, parse_pick};
use super::refs::{member_models_out, refs_model_row_count};
use super::root::{require_graph, require_repo};

// `read`. Check order is `refs`' own: flag conflict, missing query,
// `require_repo`, graph-present. The resolution IS refs' --
// `build_read_model` wraps `build_refs_model` and changes nothing about how
// a name becomes an answer -- so the ambiguity and zero-hit discipline
// cannot drift between the two verbs; only the resolved arm grows the
// declaration span.
const READ_USAGE: &str =
    "usage: devscout read <symbol> [--no-guess] [--no-dispatch] [--pick N] [--json|--compact]";

pub(crate) fn cmd_read(cwd: &Path, args: &[String]) -> (i32, String, Option<&'static str>) {
    let start = Instant::now();
    let (json, compact) = match output_flags("read", args) {
        Ok(v) => v,
        Err((code, out)) => return (code, out, None),
    };
    let Ok(pick) = parse_pick(args) else {
        return (2, READ_USAGE.to_string(), None);
    };
    let Some(q) = first_positional(args) else {
        return (2, READ_USAGE.to_string(), None);
    };
    let root = match require_repo(cwd) {
        Ok(r) => r,
        Err(e) => return (1, format!("error: {e}"), None),
    };
    let g = match require_graph(&root) {
        Ok(g) => g,
        Err((code, out)) => return (code, out, None),
    };
    let index = query::load_graph_index_with(&g, &root, index_options(args));

    // Same `--pick` narrowing rule `cmd_refs` applies: only a `MemberAmbiguous`
    // answer is ever re-resolved, against the synthetic `Owner.Member` seed the
    // picked candidate names.
    let result = match query::build_read_model(&index, q) {
        query::ReadResult::MemberAmbiguous(candidates) => match pick {
            Some(n) if n <= candidates.len() => {
                query::build_read_model(&index, &query::qualified_seed(&candidates[n - 1]))
            }
            Some(_) => return (2, READ_USAGE.to_string(), None),
            None => query::ReadResult::MemberAmbiguous(candidates),
        },
        other => other,
    };

    let answer = match result {
        query::ReadResult::NotFound => {
            let (code, out) = fallback_advised_out(q, json, format!("no symbol matches \"{q}\""));
            (code, out, query::Outcome::FallbackAdvised, 0)
        }
        query::ReadResult::Ambiguous(ids) => {
            let count = ids.len();
            let (code, out) = ambiguous_candidates_out(&index, q, &ids, json);
            (code, out, query::Outcome::Ambiguous, count)
        }
        query::ReadResult::MemberAmbiguous(candidates) => {
            let count = candidates.len();
            let (code, out) = member_ambiguous_out(q, &candidates, json);
            (code, out, query::Outcome::Ambiguous, count)
        }
        // `read`'s member answer IS refs' -- a member carries no
        // declaration-span fact to add -- so it takes the same exit code and
        // the same word, rather than a second shape to keep in step.
        query::ReadResult::Members(models) => member_models_out(q, json, compact, &models),
        query::ReadResult::Resolved(model) => {
            let count = refs_model_row_count(&model.refs);
            let out = if json {
                query::json::read_model_to_json(&model)
            } else if compact {
                render::render_read_compact(&model)
            } else {
                render::render_read_text(&model)
            };
            (0, out, query::Outcome::Hit, count)
        }
    };
    let note = (answer.2 != query::Outcome::ZeroHit).then_some(ZERO_HIT_READ);
    let (code, out) = finish_query(&root, "read", q, start, answer);
    (code, out, note)
}
