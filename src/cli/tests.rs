// The `--json` builders (`J`, `refs_model_to_json`, `impact_model_to_json`,
// ...) moved to `query::json` to keep this file under its size ratchet;
// the tests pinning their byte shape stayed here rather than following
// them, so this brings the moved names back into scope unqualified.
use crate::query::json::*;

use std::path::PathBuf;

use crate::graph;
use crate::query;
use crate::store;

use super::admin::{cmd_clear, js_math_round};
use super::answer::EXIT_NO_RESULT;
use super::args::parse_int_js;
use super::find::cmd_find;

mod admin;
mod answer;
mod args;
mod coverage;
mod find;
mod impact;
mod read;
mod refs;

// `--json` key ORDER, which a parsed-object comparison cannot see (object key
// order is not observable there). These pin three placements: a refs row's
// `heuristic` appended LAST and absent when precise, an impact row's
// `heuristicCount` then `heuristic` appended after `score` and both absent
// when precise, and `heuristicAffected` appended after `manifestGap` on the
// model itself.

fn json_refs_model(rows: Vec<query::InboundRow>, out: bool) -> query::RefsModel {
    let empty_in = || query::Table {
        total: 0,
        dropped: 0,
        rows: Vec::<query::InboundRow>::new(),
    };
    let empty_out = || query::Table {
        total: 0,
        dropped: 0,
        rows: Vec::<query::OutboundRow>::new(),
    };
    query::RefsModel {
        query: "Widget".to_string(),
        id: "App.Widget".to_string(),
        kind: "class".to_string(),
        sites: vec![query::DefSite {
            file: "src/Widget.cs".to_string(),
            line: 3,
        }],
        inbound: query::InboundTables {
            inherits: empty_in(),
            uses_type: empty_in(),
            uses_member: query::Table {
                total: rows.len(),
                dropped: 0,
                rows,
            },
            implements: empty_in(),
            overrides: empty_in(),
        },
        outbound: out.then(|| query::OutboundTables {
            inherits: empty_out(),
            uses_type: empty_out(),
            uses_member: empty_out(),
            implements: empty_out(),
            overrides: empty_out(),
            imports: query::Table {
                total: 0,
                dropped: 0,
                rows: Vec::new(),
            },
        }),
        ambiguous: query::AmbiguousTables {
            inbound: query::Table {
                total: 0,
                dropped: 0,
                rows: Vec::new(),
            },
            outbound: query::Table {
                total: 0,
                dropped: 0,
                rows: Vec::new(),
            },
        },
        bus: query::Table {
            total: 0,
            dropped: 0,
            rows: Vec::<query::BusHopRow>::new(),
        },
        manifest_gap: 0,
        member_refs: None,
    }
}
