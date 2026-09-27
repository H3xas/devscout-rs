// How many of an admitted artifact's compilations were loaded whole, read
// from the envelope the producer embeds (`context.envelope.compilations`).
// Admission accepts an artifact whose compilations are partial, and an
// occurrence from a partial compilation confirms nothing, so a coverage or
// lane report that ignored this count claimed more than the artifact holds.
//
// One definition serves `compiler-facts import|run|status` and the `map`
// lane record, so the two can never count differently. Pure, no I/O.

use std::collections::BTreeMap;

use serde_json::Value;

/// Reason token for a counted compilation that is not complete but names no
/// reason of its own.
const UNSPECIFIED: &str = "unspecified";

/// The counted compilations of one artifact.
///
/// A compilation is counted when its `state` is `complete`, `partial` or
/// `failed`. `unsupported` and `excluded` carry no facts by design, and an
/// entry with no string `state` says nothing either way, so none of them is
/// counted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompilationTally {
    total: usize,
    incomplete_by_reason: BTreeMap<String, usize>,
}

impl CompilationTally {
    /// Tallies the envelope compilations of a parsed artifact. An artifact
    /// with no envelope list tallies nothing.
    pub fn from_artifact(root: &Value) -> Self {
        let entries = root
            .get("context")
            .and_then(|c| c.get("envelope"))
            .and_then(|e| e.get("compilations"))
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        Self::from_states(entries.iter().filter_map(|entry| {
            let state = entry.get("state")?.as_str()?;
            let reason = entry.get("reason").and_then(Value::as_str).unwrap_or("");
            Some((state, reason))
        }))
    }

    /// Tallies `(state, reason)` pairs.
    pub fn from_states<'a>(states: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut tally = CompilationTally::default();
        for (state, reason) in states {
            match state {
                "complete" => tally.total += 1,
                "partial" | "failed" => {
                    tally.total += 1;
                    let reason = if reason.is_empty() {
                        UNSPECIFIED
                    } else {
                        reason
                    };
                    *tally
                        .incomplete_by_reason
                        .entry(reason.to_string())
                        .or_insert(0) += 1;
                }
                _ => {}
            }
        }
        tally
    }

    /// `M`: every counted compilation.
    pub fn total(&self) -> usize {
        self.total
    }

    /// `K`: the counted compilations that are not complete.
    pub fn partial(&self) -> usize {
        self.incomplete_by_reason.values().sum()
    }

    /// Whether at least one compilation is counted and none of them is
    /// complete.
    pub fn none_complete(&self) -> bool {
        self.total > 0 && self.partial() == self.total
    }

    /// `<reason> <n>` for each reason among the incomplete compilations,
    /// joined by `, `, in ascending byte order of the reason.
    pub fn text(&self) -> String {
        self.incomplete_by_reason
            .iter()
            .map(|(reason, n)| format!("{reason} {n}"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// `K of M compilations: <tally>`, or `None` when every counted
    /// compilation is complete.
    pub fn summary(&self) -> Option<String> {
        (self.partial() > 0).then(|| {
            format!(
                "{} of {} compilations: {}",
                self.partial(),
                self.total,
                self.text()
            )
        })
    }

    /// `K of M compilations partial: <tally>`, the lane record's detail, or
    /// `None` when every counted compilation is complete.
    pub fn detail(&self) -> Option<String> {
        (self.partial() > 0).then(|| {
            format!(
                "{} of {} compilations partial: {}",
                self.partial(),
                self.total,
                self.text()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn complete_partial_and_failed_are_counted_and_every_other_state_is_not() {
        let tally = CompilationTally::from_states([
            ("complete", "complete"),
            ("partial", "workspace-failure"),
            ("failed", "workspace-failure"),
            ("unsupported", "no-such-target"),
            ("excluded", "not-requested"),
            ("pending", "unknown"),
            ("Complete", "complete"),
        ]);
        assert_eq!(tally.total(), 3);
        assert_eq!(tally.partial(), 2);
        assert_eq!(tally.text(), "workspace-failure 2");
    }

    #[test]
    fn an_empty_reason_tallies_as_unspecified() {
        let tally = CompilationTally::from_states([("partial", ""), ("failed", "binding-error")]);
        assert_eq!(tally.text(), "binding-error 1, unspecified 1");
    }

    #[test]
    fn reasons_are_listed_in_ascending_byte_order() {
        let tally = CompilationTally::from_states([
            ("partial", "workspace-failure"),
            ("partial", "unrestored"),
            ("partial", "binding-error"),
            ("partial", "workspace-failure"),
            ("partial", "Zeta"),
        ]);
        assert_eq!(
            tally.text(),
            "Zeta 1, binding-error 1, unrestored 1, workspace-failure 2"
        );
    }

    #[test]
    fn summary_and_detail_appear_only_with_an_incomplete_compilation() {
        let healthy = CompilationTally::from_states([("complete", "complete")]);
        assert_eq!(healthy.summary(), None);
        assert_eq!(healthy.detail(), None);
        assert!(!healthy.none_complete());

        let partial = CompilationTally::from_states([
            ("complete", "complete"),
            ("complete", "complete"),
            ("complete", "complete"),
            ("partial", "workspace-failure"),
            ("partial", "workspace-failure"),
            ("partial", "workspace-failure"),
        ]);
        assert_eq!(
            partial.summary().as_deref(),
            Some("3 of 6 compilations: workspace-failure 3")
        );
        assert_eq!(
            partial.detail().as_deref(),
            Some("3 of 6 compilations partial: workspace-failure 3")
        );
        assert!(!partial.none_complete());
    }

    #[test]
    fn none_complete_needs_at_least_one_counted_compilation() {
        assert!(!CompilationTally::default().none_complete());
        assert!(!CompilationTally::from_states([("excluded", "not-requested")]).none_complete());
        assert!(CompilationTally::from_states([("failed", "workspace-failure")]).none_complete());
    }

    #[test]
    fn the_artifact_tally_reads_only_the_envelope_compilations() {
        let root = json!({
            "context": {"envelope": {"compilations": [
                {"identity": {"projectName": "A"}, "state": "complete", "reason": "complete"},
                {"identity": {"projectName": "B"}, "state": "partial", "reason": "workspace-failure"},
                {"identity": {"projectName": "C"}, "state": "partial"},
                {"identity": {"projectName": "D"}},
                {"identity": {"projectName": "E"}, "state": 3, "reason": "binding-error"},
                "not an object",
            ]}},
            "compilations": [{"state": "partial", "reason": "elsewhere"}],
        });
        let tally = CompilationTally::from_artifact(&root);
        assert_eq!(tally.total(), 3);
        assert_eq!(tally.text(), "unspecified 1, workspace-failure 1");
    }

    #[test]
    fn an_artifact_without_an_envelope_list_tallies_nothing() {
        for root in [
            json!({}),
            json!({"context": {"envelope": {}}}),
            json!({"context": {"envelope": {"compilations": {}}}}),
            json!([]),
        ] {
            assert_eq!(
                CompilationTally::from_artifact(&root),
                CompilationTally::default()
            );
        }
    }
}
