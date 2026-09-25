// `devscout audit --semantic <refs.jsonl>` -- scores this repo's `uses-member`
// graph edges against a Roslyn-derived oracle (`tools/scout-semantic`, whose
// own README documents its record schema). The oracle emits one
// ground-truth record
// per member reference in a compiled solution; this command joins those
// records to the graph's own `uses-member` edges on `(file, startLine)`, AND
// on `member` when the edge names one (schema 2's `member` key, joined
// against the oracle's own `member` field -- two references sharing a line,
// e.g. a fluent chain's qualifier call and its own member access, are not
// interchangeable evidence for one another), and reports precision/recall
// per resolver tier, plus a handful of leak/structural-impossibility signals
// that catch a specific class of resolver bug (a guessed edge landing on a
// member the caller's project can never actually see).
//
// Split by concern, mirroring the `graph.rs`/`resolve.rs` split: `model.rs`
// carries the shared row/`Inputs` shapes; `load.rs` touches the filesystem
// (graph.json, the oracle JSONL files, the manifest) and returns `Inputs`;
// `scoring.rs` carries the pure matching/structural-check primitives;
// `report.rs` carries the per-tier counters; `score.rs` is a pure function
// from `Inputs` to `AuditReport` with no I/O at all, so every scoring rule
// is unit-testable without a repo on disk; `render.rs` renders text/`--json`;
// `assert_check.rs` evaluates `--assert` thresholds; `cmd.rs` is the CLI
// entry point. `graph.json` is read as a bare `serde_json::Value`, not
// through `graph::read_graph` -- parsing the loose `Value` here means a
// future schema addition (the `source: Option<Provenance>` slot `graph.rs`
// reserves after `member`) requires no change to this module's load path,
// only to `load::tier_of`/`load::parse_graph`.
//
// `OracleRef` is trimmed to the fields the scoring rules actually consult
// (file/startLine/shape/receiverKind/member/target/targetKind/targetFile/
// external/ambiguous) -- `line`, `ext`, `receiver`, `receiverText`,
// `memberKind`, `targetUnit` and `unit` are part of the oracle's on-disk
// schema but never referenced by any rule here, so declaring them would only
// be dead weight (unknown JSON keys are ignored by serde without
// `deny_unknown_fields`, so dropping them from the struct changes nothing
// about what a real refs.jsonl file parses to).

mod assert_check;
mod cmd;
mod fp_sites;
mod load;
mod model;
mod render;
mod report;
mod score;
mod scoring;

pub(crate) use cmd::cmd_audit;

#[cfg(test)]
mod tests;
