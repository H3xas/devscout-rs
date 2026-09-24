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
// builder means the seed already resolved.
pub(crate) fn tests_model_to_json(model: &query::TestsModel) -> String {
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
                        // when the row's vouch is the project model: an
                        // attribute-vouched row keeps today's exact bytes.
                        let why = if r.via == query::TestVia::Project {
                            fields.push(("via", J::Str("project".to_string())));
                            Why::TestProject
                        } else {
                            Why::TestAttribute
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
    fields.push(("outcome", J::Str(query::Outcome::Hit.as_str().to_string())));
    J::Obj(fields).to_json_string()
}
