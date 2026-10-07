// The `next` key: the same suggestion lists the text trailer prints, in the
// argv-array shape the answer contract states them in.
//
// Each builder here reuses its verb's own field list from `json.rs` and swaps
// the trailing `outcome` field for `next` followed by `outcome`, so the key
// lands immediately before `outcome` in every shape without a second
// hand-written field list to keep in step.

use crate::graph;
use crate::query;

use super::{
    impact_model_fields, impact_outcome, imported_section_fields, j_next, member_refs_fields,
    read_model_fields, refs_model_fields, tests_model::tests_model_fields, J,
};

// `next` sits immediately before `outcome`, which is the trailing field of
// every answer object these builders are handed. The key is checked rather
// than assumed: a field list that ends in something else would silently take
// `next` into a slot the contract does not put it in.
fn with_next(mut fields: Vec<(&'static str, J)>, next: &[Vec<String>]) -> Vec<(&'static str, J)> {
    let outcome = fields
        .pop()
        .filter(|(key, _)| *key == "outcome")
        .expect("`next` is placed by swapping the trailing `outcome` field");
    fields.push(("next", j_next(next)));
    fields.push(outcome);
    fields
}

fn refs_j_with_next(model: &query::RefsModel, outcome: query::Outcome, next: &[Vec<String>]) -> J {
    J::Obj(with_next(refs_model_fields(model, outcome), next))
}

/// The resolved `refs` shape, with `next` immediately before `outcome`.
pub(crate) fn refs_model_to_json_with_next(
    model: &query::RefsModel,
    next: &[Vec<String>],
) -> String {
    refs_j_with_next(model, query::Outcome::Hit, next).to_json_string()
}

/// The resolved `read` shape, with `next` immediately before `outcome`.
pub(crate) fn read_model_to_json_with_next(
    model: &query::ReadModel,
    next: &[Vec<String>],
) -> String {
    J::Obj(with_next(read_model_fields(model), next)).to_json_string()
}

/// The resolved `tests` shape, with `next` immediately before `outcome`.
pub(crate) fn tests_model_to_json_with_next(
    model: &query::TestsModel,
    next: &[Vec<String>],
) -> String {
    J::Obj(with_next(tests_model_fields(model), next)).to_json_string()
}

// `impact`'s field list stops before `outcome`, because the caller decides
// which outcome applies -- so the two builders below append `next` and then
// `outcome` themselves rather than swapping a field that is not there yet.
fn impact_j_with_next(
    mut fields: Vec<(&'static str, J)>,
    outcome: query::Outcome,
    next: &[Vec<String>],
) -> J {
    fields.push(("next", j_next(next)));
    fields.push(("outcome", J::Str(outcome.as_str().to_string())));
    J::Obj(fields)
}

/// The resolved `impact` shape, with `next` immediately before `outcome`.
pub(crate) fn impact_model_to_json_with_next(
    query_str: &str,
    model: &query::ImpactModel,
    next: &[Vec<String>],
) -> String {
    let fields = impact_model_fields(query_str, model);
    impact_j_with_next(fields, impact_outcome(model, false), next).to_json_string()
}

/// The resolved `impact` shape with an import configured, with `next`
/// immediately before `outcome` -- after the imported section's own keys, the
/// same slot the plain shape puts it in.
pub(crate) fn impact_model_to_json_with_imports_and_next(
    query_str: &str,
    model: &query::ImpactModel,
    imported: &query::ImportedSection,
    provenance: &graph::Provenance,
    next: &[Vec<String>],
) -> String {
    let fields: Vec<(&'static str, J)> = impact_model_fields(query_str, model)
        .into_iter()
        .chain(imported_section_fields(imported, provenance))
        .collect();
    let outcome = impact_outcome(model, !imported.rows.is_empty());
    impact_j_with_next(fields, outcome, next).to_json_string()
}

/// The bare-member shape: every member object carries its own `next`, and the
/// wrapper carries what the members' lists amount to together. The wrapper is
/// itself a resolved answer, so it states its own suggestions rather than
/// leaving a reader to union the members by hand.
pub(crate) fn member_refs_to_json_with_next(
    query_str: &str,
    models: &[query::RefsModel],
    outcome: query::Outcome,
    members_next: &[Vec<Vec<String>>],
    wrapper_next: &[Vec<String>],
) -> String {
    let members: Vec<J> = models
        .iter()
        .zip(members_next)
        .map(|(model, next)| refs_j_with_next(model, outcome, next))
        .collect();
    J::Obj(member_refs_fields(
        query_str,
        members,
        Some(wrapper_next),
        outcome,
    ))
    .to_json_string()
}
