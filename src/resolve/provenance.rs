// Which emission arm answered each `uses-member` edge, recorded only when
// `SCOUT_EDGE_PROVENANCE` names an output path.
//
// The recording is deliberately downstream of every decision: an arm notes
// its step AFTER it has already pushed its edge, so nothing here can reach
// the answer. With the variable unset `note` returns before it touches
// anything and no file is opened, which is what keeps a graph built under
// observation byte-identical to one built without it.
//
// Rows are keyed by edge identity rather than by position in `edges`,
// because the heuristic dedup pass that runs after the last arm removes
// entries and shifts every index behind them.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::graph::{Edge, HeuristicTier};

/// The emission arm an edge came from, in the order the ladder reaches them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    /// A `base.`-qualified member, answered from the enclosing type's own
    /// bases and never from the type itself.
    BaseMember,
    /// A qualifier that resolved as a type, binding the def that declares
    /// the member.
    QualifierMember,
    /// A qualifier that resolved as a type, binding through its base closure
    /// or on type certainty alone.
    QualifierType,
    /// A receiver whose own type the extractor recorded, widened to its
    /// bases.
    TypedReceiver,
    /// A receiver reached through the declared type of the property that
    /// owns it.
    PropertyHop,
    /// The extension tier's sole surviving candidate.
    Extension,
    /// The scored tier's ranked guess.
    Scored,
}

impl Step {
    fn name(self) -> &'static str {
        match self {
            Step::BaseMember => "base-member",
            Step::QualifierMember => "qualifier-member",
            Step::QualifierType => "qualifier-type",
            Step::TypedReceiver => "typed-receiver",
            Step::PropertyHop => "property-hop",
            Step::Extension => "extension",
            Step::Scored => "scored",
        }
    }
}

/// The edge fields that identify one emission. `to_file` is carried because
/// two same-named defs in different files are different answers.
type Key = (String, usize, String, String, Option<String>);

fn key(e: &Edge) -> Option<Key> {
    match e {
        Edge::UsesMember {
            from_file,
            from_line,
            to,
            to_file,
            member,
            ..
        } => Some((
            from_file.clone(),
            *from_line,
            to.clone(),
            to_file.clone(),
            member.clone(),
        )),
        _ => None,
    }
}

fn out_path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| std::env::var_os("SCOUT_EDGE_PROVENANCE").map(PathBuf::from))
        .as_ref()
}

fn rows() -> &'static Mutex<HashMap<Key, Step>> {
    static ROWS: OnceLock<Mutex<HashMap<Key, Step>>> = OnceLock::new();
    ROWS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Records the step that emitted the edge just pushed onto `edges`. A no-op
/// unless an output path is set.
pub(super) fn note(edges: &[Edge], step: Step) {
    if out_path().is_none() {
        return;
    }
    let Some(k) = edges.last().and_then(key) else {
        return;
    };
    if let Ok(mut map) = rows().lock() {
        // First arm to claim an identity keeps it: a repeated precise edge is
        // a repeated reference on one line, not a second answer to the first.
        map.entry(k).or_insert(step);
    }
}

/// Writes one JSON object per surviving `uses-member` edge, in graph order.
/// An edge no arm claimed carries `"step": null`, which is the signal that
/// the recorded ladder does not describe everything the resolver emits.
pub(super) fn flush(edges: &[Edge]) {
    let Some(path) = out_path() else {
        return;
    };
    let Ok(map) = rows().lock() else {
        return;
    };
    let mut body = String::new();
    for e in edges {
        let Some(k) = key(e) else {
            continue;
        };
        let step = match map.get(&k) {
            Some(s) => format!("\"{}\"", s.name()),
            None => "null".to_string(),
        };
        let tier = match e.tier() {
            Some(HeuristicTier::Ext) => "\"ext\"",
            Some(HeuristicTier::Guess) => "\"guess\"",
            None => "\"precise\"",
        };
        let member = match &k.4 {
            Some(m) => escape(m),
            None => "null".to_string(),
        };
        body.push_str(&format!(
            "{{\"file\":{},\"line\":{},\"to\":{},\"to_file\":{},\"member\":{},\"tier\":{},\"step\":{}}}\n",
            escape(&k.0),
            k.1,
            escape(&k.2),
            escape(&k.3),
            member,
            tier,
            step
        ));
    }
    if let Ok(mut f) = std::fs::File::create(path) {
        let _ = f.write_all(body.as_bytes());
    }
}

/// A quoted, fully-escaped JSON string, control characters included --
/// `serde_json`'s own `str` encoding, which is the same encoder
/// `src/audit/fp_sites.rs` renders its rows through. A raw control
/// character (a bare newline or tab inside a path or member name) previously
/// passed through unescaped here and would have produced invalid JSONL; a
/// `&str` always serializes to a JSON string with no error path, so this
/// never fails.
fn escape(s: &str) -> String {
    serde_json::to_string(s).expect("a &str always serializes to a JSON string")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A raw control character in a path or member name used to reach the
    /// JSONL body unescaped and produce an invalid line; `escape` now runs
    /// every string through `serde_json`, which turns it into a valid
    /// two-character JSON escape and keeps the round trip lossless.
    #[test]
    fn escape_turns_a_raw_control_character_into_a_valid_json_escape() {
        let out = escape("line\none\ttwo");
        assert_eq!(out, "\"line\\none\\ttwo\"");
        let parsed: serde_json::Value =
            serde_json::from_str(&out).expect("escape must produce valid JSON");
        assert_eq!(parsed, "line\none\ttwo");
    }

    /// The two characters the old hand-rolled escape already covered stay
    /// covered by the `serde_json` encoder.
    #[test]
    fn escape_still_covers_quote_and_backslash() {
        let out = escape("a\"b\\c");
        assert_eq!(out, "\"a\\\"b\\\\c\"");
    }
}
