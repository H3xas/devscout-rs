// The `--pick` narrowing `refs`, `read`, `impact` and `tests` share.
//
// One helper serves both forms on purpose. `--pick <n>` hands back candidate
// `n`'s own answer, and section `n` of `--pick all` is that same answer, so
// the two cannot drift: they are the same call with the same seed and the
// same verb renderer behind it.
//
// Sections are never merged, deduplicated or reordered against each other.
// Two candidates of one member name reach different declarations and
// therefore different callers, and a caller who asked for every candidate
// asked for exactly that -- a union would hide which candidate a row came
// from.

use crate::exit::{EXIT_NO_RESULT, EXIT_OK, EXIT_USAGE};
use crate::query;

use super::answer::{member_candidate_object, type_candidate_object};
use super::args::Pick;

/// One candidate's rendered answer, before `finish_query` splices freshness
/// and records the invocation's telemetry event.
///
/// `out` is the bytes the verb would print for this candidate alone, in the
/// mode the caller asked for; `next` is the suggestion list those bytes carry
/// as a `next:` trailer or a `next` key.
pub(crate) struct SectionAnswer {
    pub(crate) code: i32,
    pub(crate) out: String,
    pub(crate) outcome: query::Outcome,
    pub(crate) count: usize,
    pub(crate) next: Vec<Vec<String>>,
}

impl SectionAnswer {
    pub(crate) fn new(
        code: i32,
        out: String,
        outcome: query::Outcome,
        count: usize,
        next: Vec<Vec<String>>,
    ) -> SectionAnswer {
        SectionAnswer {
            code,
            out,
            outcome,
            count,
            next,
        }
    }

    /// The four facts `finish_query` takes, so a narrowed answer and a base
    /// answer reach it through one shape.
    pub(crate) fn into_tuple(self) -> (i32, String, query::Outcome, usize) {
        (self.code, self.out, self.outcome, self.count)
    }
}

/// The candidates an ambiguous answer refused between, in the order the
/// refusal prints them, each paired with the seed its own answer starts from.
///
/// `query` is the seed as the caller typed it, which is what every answer of
/// the invocation echoes -- the sections wrapper included.
pub(crate) struct Candidates {
    query: String,
    rows: Vec<String>,
    objects: Vec<query::json::J>,
    picked: Vec<query::PickedSeed>,
}

impl Candidates {
    /// The rows of a member ambiguity, in the order the refusal prints them.
    pub(crate) fn from_members(q: &str, candidates: &[query::MemberCandidate]) -> Candidates {
        Candidates {
            query: q.to_string(),
            rows: candidates.iter().map(query::member_candidate_row).collect(),
            objects: candidates.iter().map(member_candidate_object).collect(),
            picked: candidates
                .iter()
                .cloned()
                .map(query::PickedSeed::Member)
                .collect(),
        }
    }

    /// The rows of a type ambiguity, in the order the refusal prints them --
    /// taken from the refusal's own ordering, so `--pick n` answers the row a
    /// caller read at line `n`.
    pub(crate) fn from_types(q: &str, index: &query::GraphIndex, ids: &[String]) -> Candidates {
        let ordered = query::type_candidates_in_refusal_order(index, ids);
        Candidates {
            query: q.to_string(),
            rows: ordered
                .iter()
                .map(|id| query::type_candidate_row(id, query::candidate_def(index, id)))
                .collect(),
            objects: ordered
                .iter()
                .map(|id| type_candidate_object(index, id))
                .collect(),
            picked: ordered.into_iter().map(query::PickedSeed::Type).collect(),
        }
    }
}

/// Answer `pick` against the candidates an ambiguous seed produced.
///
/// `answer_for` is the verb's own per-candidate builder: it takes the seed a
/// candidate starts from and returns that candidate's answer. Every form
/// below goes through it, which is what keeps `--pick n` and section `n`
/// byte-identical.
///
/// `Err` is the verb's usage error: `Pick::Nth` outside the candidate list,
/// which is the same mistake a member candidate already made at the base.
pub(crate) fn narrow<F>(
    candidates: Candidates,
    pick: Pick,
    json: bool,
    usage: &'static str,
    answer_for: F,
) -> Result<SectionAnswer, (i32, String)>
where
    F: Fn(&query::PickedSeed) -> SectionAnswer,
{
    match pick {
        Pick::Nth(n) => match candidates.picked.get(n - 1) {
            Some(picked) => Ok(answer_for(picked)),
            None => Err((EXIT_USAGE, usage.to_string())),
        },
        Pick::All => Ok(all_sections(&candidates, json, &answer_for)),
    }
}

// Every candidate answered, then kept apart. The wrapper's outcome is `hit`
// when at least one section is, and `zero-hit` otherwise -- the rule the
// bare-member wrapper applies to its members -- so the exit code and the
// stderr note describe the answer as a whole. The row count is the sum of the
// sections', and the suggestions are their union, by the same merge the
// bare-member wrapper uses.
fn all_sections<F>(candidates: &Candidates, json: bool, answer_for: &F) -> SectionAnswer
where
    F: Fn(&query::PickedSeed) -> SectionAnswer,
{
    let answers: Vec<SectionAnswer> = candidates.picked.iter().map(answer_for).collect();
    let outcome = if answers.iter().any(|a| a.outcome == query::Outcome::Hit) {
        query::Outcome::Hit
    } else {
        query::Outcome::ZeroHit
    };
    let code = if outcome == query::Outcome::Hit {
        EXIT_OK
    } else {
        EXIT_NO_RESULT
    };
    let count = answers.iter().map(|a| a.count).sum();
    let section_next: Vec<Vec<Vec<String>>> = answers.iter().map(|a| a.next.clone()).collect();
    let next = query::merge_suggestions(section_next);
    let out = if json {
        let answers: Vec<&str> = answers.iter().map(|a| a.out.as_str()).collect();
        query::json::sections::sections_to_json(
            &candidates.query,
            &candidates.objects,
            &answers,
            &next,
            outcome,
        )
    } else {
        sections_text(candidates, &answers)
    };
    SectionAnswer::new(code, out, outcome, count, next)
}

// The section bodies, each under its own label line and separated by one
// blank line. Every body is a complete answer, so it carries its own `next:`
// trailer and nothing follows the last section.
fn sections_text(candidates: &Candidates, answers: &[SectionAnswer]) -> String {
    let n = answers.len();
    answers
        .iter()
        .enumerate()
        .map(|(i, a)| {
            format!(
                "candidate {} of {n}: {}\n{}",
                i + 1,
                candidates.rows[i],
                a.out
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
