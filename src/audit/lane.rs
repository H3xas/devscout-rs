// The lane `audit` reports for the graph it scores, read from graph.json's
// `stats.lane` record when `map` wrote one and from `stats.semantic`
// otherwise. A run whose facts came partly from incomplete compilations used
// to read as plain `enriched`; it now reads `enriched-partial`, which a check
// for `enriched` rejects. A graph written before `stats.lane` existed reads
// exactly as it did.

use serde_json::Value;

use crate::query::json::J;

const PARTIAL: &str = "partial";
const NOT_APPLIED: &str = "not-applied";

/// The label and the `stats.lane` record it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditLane {
    label: &'static str,
    record: Option<Value>,
}

impl AuditLane {
    /// A syntax-lane graph with no lane record, for reports built by hand.
    #[cfg(test)]
    pub const SYNTAX: AuditLane = AuditLane {
        label: "syntax",
        record: None,
    };

    /// `enriched-partial` when `stats.lane.state` is `partial`, else
    /// `enriched` when `stats.semantic` is present, else `syntax`.
    pub fn read(graph: &Value) -> AuditLane {
        let stats = graph.get("stats");
        let record = stats.and_then(|s| s.get("lane")).cloned();
        let state = record.as_ref().and_then(|r| r.get("state")?.as_str());
        let label = if state == Some(PARTIAL) {
            "enriched-partial"
        } else if stats.and_then(|s| s.get("semantic")).is_some() {
            "enriched"
        } else {
            "syntax"
        };
        AuditLane { label, record }
    }

    /// The `lane` value of the report.
    pub fn label(&self) -> &'static str {
        self.label
    }

    /// The text header's lane token: the label, followed by why the lane is
    /// partial or not applied when the record says so.
    pub fn header(&self) -> String {
        let Some(record) = &self.record else {
            return self.label.to_string();
        };
        let field = |key: &str| record.get(key).and_then(Value::as_str);
        match (field("state"), field("reason"), field("detail")) {
            (Some(PARTIAL), _, Some(detail)) => format!("{} ({detail})", self.label),
            (Some(NOT_APPLIED), Some(reason), Some(detail)) => {
                format!(
                    "{} (enriched lane not applied: {reason}: {detail})",
                    self.label
                )
            }
            (Some(NOT_APPLIED), Some(reason), None) => {
                format!("{} (enriched lane not applied: {reason})", self.label)
            }
            _ => self.label.to_string(),
        }
    }

    /// `lane_detail` (the record verbatim) and `compilations` (the record's
    /// counts), each only when present, in that order.
    pub fn json_fields(&self) -> Vec<(&'static str, J)> {
        let Some(record) = &self.record else {
            return Vec::new();
        };
        let mut fields = vec![("lane_detail", raw(record))];
        if let Some(compilations) = record.get("compilations") {
            fields.push(("compilations", raw(compilations)));
        }
        fields
    }
}

/// `J` has no variant for an arbitrary JSON value; `RawNum` writes its text
/// through unchanged, and `serde_json`'s own encoding of a value is valid JSON.
fn raw(value: &Value) -> J {
    J::RawNum(value.to_string())
}
