// `tests_model_to_json`'s own home, split out of `json.rs` to keep that file
// under its flat line limit -- see `json.rs`'s module doc for the JSON
// builder pipeline this slots into. `pub(super)` throughout: only `json.rs`
// itself re-exports this module's public entry point.

use crate::query;
use crate::query::why::Why;

use super::{j_bus_row, j_table, push_heuristic, J};

// The resolved `tests` JSON shape (`build_tests_model`'s resolved return):
// `{status, query, symbol, defFiles, rows, testFileCount, refCount,
// heuristicFileCount, heuristicRefCount, outcome}`, in that key order, with
// the heuristic pair before `outcome`, always last. Each row carries
// `via: "project"` as its own last key, appended after `heuristic`/`tier`,
// ONLY when the row's vouch is the project model -- an attribute-vouched row
// emits no `via` key at all, so today's bytes for every graph without a
// project model are unchanged. `outcome` is always `"hit"`: reaching this
// builder means the seed already resolved. An optional `nameHint` sits after
// `heuristicRefCount` and before `outcome`, present only on the zero case
// with a matching test file, so every other answer keeps its bytes.
#[cfg(test)]
#[cfg(test)]
pub(crate) fn tests_model_to_json(model: &query::TestsModel) -> String {
    J::Obj(tests_model_fields(model)).to_json_string()
}

// The resolved `tests` field list, `outcome` last. Shared with `json/next.rs`,
// which splices `next` into the slot before `outcome` -- so the trailing field
// is load-bearing, not just the end of the list.
pub(super) fn tests_model_fields(model: &query::TestsModel) -> Vec<(&'static str, J)> {
    let mut fields: Vec<(&'static str, J)> = vec![
        ("schema_version", J::UInt(query::SCHEMA_VERSION)),
        ("status", J::Str("resolved".to_string())),
        ("query", J::Str(model.query.clone())),
        ("symbol", J::Str(model.symbol.clone())),
        (
            "defFiles",
            J::Arr(model.def_files.iter().map(|f| J::Str(f.clone())).collect()),
        ),
        (
            "rows",
            J::Arr(
                model
                    .rows
                    .iter()
                    .map(|r| {
                        let mut fields = vec![
                            ("file", J::Str(r.file.clone())),
                            (
                                "testDefs",
                                J::Arr(r.test_defs.iter().map(|d| J::Str(d.clone())).collect()),
                            ),
                            (
                                "lines",
                                J::Arr(r.lines.iter().map(|l| J::UInt(*l as u64)).collect()),
                            ),
                            ("refCount", J::UInt(r.ref_count as u64)),
                        ];
                        push_heuristic(&mut fields, r.heuristic, r.tier);
                        // `via` is appended LAST, after `heuristic`/`tier`, and only
                        // when the row's vouch is the project model or the
                        // file's name: an attribute-vouched row keeps today's
                        // exact bytes.
                        let why = match r.via {
                            query::TestVia::Attribute => Why::TestAttribute,
                            query::TestVia::Project => {
                                fields.push(("via", J::Str("project".to_string())));
                                Why::TestProject
                            }
                            query::TestVia::Path => {
                                fields.push(("via", J::Str("path".to_string())));
                                Why::TestPath
                            }
                        };
                        // Appended absolute LAST, after `via` when present: which of
                        // the two ways `tests` reaches a file earned this row.
                        fields.push(("why", J::Str(why.as_str().to_string())));
                        J::Obj(fields)
                    })
                    .collect(),
            ),
        ),
        ("testFileCount", J::UInt(model.test_file_count as u64)),
        ("refCount", J::UInt(model.ref_count as u64)),
        (
            "heuristicFileCount",
            J::UInt(model.heuristic_file_count as u64),
        ),
        (
            "heuristicRefCount",
            J::UInt(model.heuristic_ref_count as u64),
        ),
    ];
    // Appended after the heuristic pair and before `outcome`, the identical
    // line `refs_model_to_json`'s own builder already has -- present only
    // when a test file publishes to this handler over a bus hop, so a graph
    // (or a symbol) with none renders byte-identical to before this field
    // existed.
    if model.bus.total != 0 {
        fields.push(("bus-hop", j_table(&model.bus, j_bus_row)));
    }
    // The name hint, when the answer is the zero case and a test file's own
    // name matches the symbol. Same additive rule as `bus-hop` above: an
    // empty hint leaves the key out, so every other answer's bytes are
    // unchanged. `next` is spliced in after it, so the key order is
    // `heuristicRefCount`, `nameHint`, `next`, `outcome`.
    if !model.name_hint.is_empty() {
        fields.push(("nameHint", name_hint_json(&model.name_hint)));
    }
    fields.push(("outcome", J::Str(query::Outcome::Hit.as_str().to_string())));
    fields
}

// `nameHint`: one object per matching file, in path order, each naming the
// production symbols that file references and how many distinct ones there
// are -- so a reader sees a capped `references` list next to its own total
// rather than having to guess whether the list is complete.
fn name_hint_json(hint: &[query::NameHintRow]) -> J {
    J::Arr(
        hint.iter()
            .map(|row| {
                J::Obj(vec![
                    ("file", J::Str(row.file.clone())),
                    (
                        "references",
                        J::Arr(row.references.iter().map(|r| J::Str(r.clone())).collect()),
                    ),
                    ("referenceTotal", J::UInt(row.reference_total as u64)),
                ])
            })
            .collect(),
    )
}
