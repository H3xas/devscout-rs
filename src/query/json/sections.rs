// The `--pick all` `--json` wrapper: one object per candidate of an
// ambiguous seed, each carrying the candidate the refusal named and that
// candidate's own answer.
//
// The list key is `sections`, never `candidates`: the refusal's `candidates`
// list is a refusal, and a consumer must not read an answer that kept every
// candidate apart as if it were that list.
//
// Key order follows the bare-member wrapper (`member_refs_fields`):
// `schema_version`, `status`, `query`, the list, `next` immediately before
// `outcome`, and `freshness` once at the end -- spliced by `finish_query`,
// which is why this writer never emits it.

use crate::query;

use super::next::j_next;
use super::J;

/// Write the `sections` wrapper.
///
/// `candidates` and `answers` are parallel slices, one entry each per
/// candidate, in the refusal's own order: the refusal's candidate object, and
/// that candidate's `--json` answer as its verb already rendered it. The
/// answers are spliced in verbatim rather than rebuilt, so a section's bytes
/// and a plain `--pick <n> --json` answer cannot drift apart. `next` is the
/// union of the sections' own lists, by the same merge the bare-member wrapper
/// uses.
pub(crate) fn sections_to_json(
    query_str: &str,
    candidates: &[J],
    answers: &[&str],
    next: &[Vec<String>],
    outcome: query::Outcome,
) -> String {
    debug_assert_eq!(
        candidates.len(),
        answers.len(),
        "every candidate carries exactly one answer"
    );
    let mut out = String::from("{\"schema_version\":");
    out.push_str(&query::SCHEMA_VERSION.to_string());
    out.push_str(",\"status\":\"sections\",\"query\":");
    out.push_str(&J::Str(query_str.to_string()).to_json_string());
    out.push_str(",\"sections\":[");
    for (i, (candidate, answer)) in candidates.iter().zip(answers).enumerate() {
        // A section answer is one complete top-level object, the same shape
        // every `--json` answer this crate renders is. Splicing anything else
        // would produce a document no parser can read, so it fails loud here
        // rather than at a consumer.
        debug_assert!(
            answer.starts_with('{') && answer.ends_with('}'),
            "a section answer must be exactly one top-level object: {answer}"
        );
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"pick\":");
        out.push_str(&(i + 1).to_string());
        out.push_str(",\"candidate\":");
        out.push_str(&candidate.to_json_string());
        out.push_str(",\"answer\":");
        out.push_str(answer);
        out.push('}');
    }
    out.push_str("],\"next\":");
    out.push_str(&j_next(next).to_json_string());
    out.push_str(",\"outcome\":");
    out.push_str(&J::Str(outcome.as_str().to_string()).to_json_string());
    out.push('}');
    out
}
