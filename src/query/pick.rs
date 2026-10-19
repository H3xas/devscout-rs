// One candidate of an ambiguous seed, answered on its own.
//
// A `--pick <n>` request and one section of a `--pick all` answer both come
// through here, so the two forms cannot disagree about a candidate: section
// `n` IS the `--pick <n>` answer, rendered by the same verb call.
//
// A member candidate resolves by its EXACT owner id and its member name,
// deliberately without the seed ladder. `qualified_seed` re-spells the
// candidate as `Owner.Member`, and the ladder's dotted-suffix rule admits
// every owner whose id ends with those segments -- so two owners that differ
// only in their leading namespaces would answer the same ambiguity again.
// The candidate is already chosen, so the suffix rule has nothing left to
// narrow.

use crate::graph;

use super::coverage::{tests_model_for_id, TestsResult};
use super::impact::{impact_model_for_seed, ImpactResult, SeedKind};
use super::index::GraphIndex;
use super::member::{exact_member_owner, qualified_seed, MemberCandidate};
use super::read::{build_read_model, read_result_from_refs, ReadResult};
use super::refs::{build_refs_model, member_refs_from_owners, MemberRefsOutcome, RefsResult};
use super::refs_tables::{DEFAULT_CAP, INBOUND_CAP, OUTBOUND_CAP};

/// Which candidate an answer starts from: a member's exact declaring type, or
/// a type's def id.
#[derive(Debug, Clone, PartialEq)]
pub enum PickedSeed {
    /// A member candidate: the declaring type's def id and the member name.
    Member(MemberCandidate),
    /// A type candidate: its def id.
    Type(String),
}

/// The refusal's own text row for one type candidate.
///
/// The refusal prints this row, a section label quotes it, and the order the
/// refusal prints its rows in is the order `--pick` numbers them -- so the
/// form lives here once, beside that order.
pub fn type_candidate_row(id: &str, d: &graph::Def) -> String {
    format!("{id}  {}:{}  {}", d.file, d.line, d.kind)
}

/// The refusal's own text row for one member candidate.
pub fn member_candidate_row(c: &MemberCandidate) -> String {
    format!("{}.{}  {}:{}", c.owner, c.name, c.file, c.line)
}

/// The type candidate ids in the order the refusal prints their rows.
///
/// Sorted on the rendered row, the same comparison the refusal itself sorts
/// by, so the candidate a caller reads at line `n` is the candidate `--pick
/// n` answers.
pub fn type_candidates_in_refusal_order(index: &GraphIndex, ids: &[String]) -> Vec<String> {
    let mut rows: Vec<(String, String)> = ids
        .iter()
        .map(|id| {
            let d = candidate_def(index, id);
            (type_candidate_row(id, d), id.clone())
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.into_iter().map(|(_, id)| id).collect()
}

/// The def an ambiguous candidate id names.
///
/// Every candidate id comes from `resolve_symbol`'s own candidate set, built
/// from `index.by_id`'s keys, so the lookup cannot miss.
pub fn candidate_def<'a>(index: &'a GraphIndex, id: &str) -> &'a graph::Def {
    index
        .def(id)
        .expect("ambiguous candidate id must resolve to a graph def")
}

/// The `refs` answer for one candidate.
pub fn pick_refs(
    index: &GraphIndex,
    picked: &PickedSeed,
    out: bool,
    cap: usize,
    inbound_cap: usize,
    outbound_cap: usize,
    all_out: bool,
) -> RefsResult {
    match picked {
        PickedSeed::Type(id) => {
            build_refs_model(index, id, out, cap, inbound_cap, outbound_cap, all_out)
        }
        PickedSeed::Member(c) => match member_refs_for_candidate(index, c, inbound_cap) {
            Some(MemberRefsOutcome::Models(models)) => RefsResult::Members(models),
            Some(MemberRefsOutcome::Ambiguous(candidates)) => {
                RefsResult::MemberAmbiguous(candidates)
            }
            None => RefsResult::NotFound,
        },
    }
}

/// The `read` answer for one candidate.
pub fn pick_read(index: &GraphIndex, picked: &PickedSeed) -> ReadResult {
    match picked {
        PickedSeed::Type(id) => build_read_model(index, id),
        PickedSeed::Member(_) => {
            let refs = pick_refs(
                index,
                picked,
                false,
                DEFAULT_CAP,
                INBOUND_CAP,
                OUTBOUND_CAP,
                false,
            );
            read_result_from_refs(index, refs)
        }
    }
}

/// The `tests` answer for one candidate.
///
/// A member candidate answers AS its declaring type with the candidate's own
/// `Owner.Member` spelling echoed back, which is what the typed-seed path
/// echoes; a type candidate echoes its def id, so this answer and the same
/// verb run on that id are the same bytes.
pub fn pick_tests(index: &GraphIndex, picked: &PickedSeed) -> TestsResult {
    match picked {
        PickedSeed::Type(id) => tests_model_for_id(index, id, id),
        PickedSeed::Member(c) => match exact_member_owner(index, &c.name, &c.owner) {
            Some((owner, _)) => tests_model_for_id(index, &owner, &qualified_seed(c)),
            None => TestsResult::NotFound,
        },
    }
}

/// The `impact` answer for one candidate.
///
/// A type candidate starts from its def id as a single resolved SYMBOL seed.
/// It cannot go through [`super::impact_seed::resolve_impact_seed`]: a dotted
/// type id reads as a file path there, so the CLI has no seed spelling for
/// this candidate at all.
pub fn pick_impact(
    index: &GraphIndex,
    picked: &PickedSeed,
    hops: u32,
    cap: usize,
    iface: bool,
    iface_max_fanin: usize,
    hub_max_indegree: usize,
) -> ImpactResult {
    let id = match picked {
        PickedSeed::Type(id) => id.clone(),
        PickedSeed::Member(c) => match exact_member_owner(index, &c.name, &c.owner) {
            Some((owner, _)) => owner,
            None => {
                return ImpactResult::NotFound {
                    kind: SeedKind::Symbol,
                }
            }
        },
    };
    impact_model_for_seed(
        index,
        SeedKind::Symbol,
        vec![id],
        hops,
        cap,
        iface,
        iface_max_fanin,
        hub_max_indegree,
    )
}

// One candidate's member answer: the owners the exact-owner lookup admits --
// zero or one, never a suffix pair -- run through the same verification,
// ranking, budget and grouping a typed `Owner.Member` seed runs.
fn member_refs_for_candidate(
    index: &GraphIndex,
    c: &MemberCandidate,
    inbound_cap: usize,
) -> Option<MemberRefsOutcome> {
    let owners = exact_member_owner(index, &c.name, &c.owner)
        .into_iter()
        .collect::<Vec<_>>();
    member_refs_from_owners(index, &qualified_seed(c), &c.name, owners, inbound_cap)
}
