// The eight-state uncertainty vocabulary the compiler-enrichment design
// names, and the one place every producer-side and consumer-owned signal
// folds into it. Only `Confirmed` ever participates in the resolver's
// same-context override (`precedence.rs`); every other state falls through
// to the syntax ladder unchanged, which is what makes "absence of a record
// is never a negative fact" true by construction rather than by a second
// check somewhere else.

/// One of the design's eight distinct, reasoned uncertainty states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Uncertainty {
    /// A same-context compiler fact names exactly one target.
    Confirmed,
    /// A resolution string this admission path does not yet recognise --
    /// forward-compatible with a producer that adds a new outcome, never
    /// silently promoted to `Confirmed`.
    Candidate,
    /// The compiler itself could not choose among more than one symbol.
    Ambiguous,
    /// The compiler found no candidate at all for this occurrence.
    Unresolved,
    /// The compiler bound the occurrence to something this consumer does not
    /// treat as a same-context answer -- dynamic dispatch (late binding) is
    /// the design's own named example.
    Unsupported,
    /// The compiler found a candidate and explicitly ruled it out
    /// (inaccessible from the call site), rather than merely failing to
    /// choose one.
    Excluded,
    /// The admitted artifact's own freshness check failed: the checkout has
    /// moved since the artifact was captured.
    Stale {
        /// The machine-readable freshness trigger -- owned because an
        /// import-changed/import-missing trigger names the specific import.
        reason: String,
    },
    /// The occurrence's own compilation is a diagnostically incomplete or
    /// unsupported unit -- a partial compile result can never be reported as
    /// a clean, confirmed answer.
    Incomplete {
        /// The compilation's own first-error or drop reason.
        reason: String,
    },
}

impl Uncertainty {
    /// A short, stable, machine-readable reason string -- every state
    /// carries one, per the design's own "each with a reason" requirement.
    pub fn reason(&self) -> String {
        match self {
            Uncertainty::Confirmed => "confirmed".to_string(),
            Uncertainty::Candidate => "unrecognised-resolution".to_string(),
            Uncertainty::Ambiguous => "compiler-ambiguous".to_string(),
            Uncertainty::Unresolved => "compiler-unresolved".to_string(),
            Uncertainty::Unsupported => "dynamic-dispatch".to_string(),
            Uncertainty::Excluded => "inaccessible".to_string(),
            Uncertainty::Stale { reason } => reason.clone(),
            Uncertainty::Incomplete { reason } => reason.clone(),
        }
    }

    /// Whether this state may participate in the resolver's same-context
    /// override. Every other state falls through to the syntax ladder
    /// untouched.
    pub fn is_confirmed(&self) -> bool {
        matches!(self, Uncertainty::Confirmed)
    }
}

/// Maps one occurrence site's own `resolution` string (the literal the
/// engine writes) to its `Uncertainty` state. Producer-recognised values
/// today: `"confirmed"`, `"ambiguous"`, `"unresolved"`, `"inaccessible"`,
/// `"dynamic"`; anything else maps to `Candidate` rather than being assumed
/// confirmed.
pub fn from_resolution(resolution: &str) -> Uncertainty {
    match resolution {
        "confirmed" => Uncertainty::Confirmed,
        "ambiguous" => Uncertainty::Ambiguous,
        "unresolved" => Uncertainty::Unresolved,
        "dynamic" => Uncertainty::Unsupported,
        "inaccessible" => Uncertainty::Excluded,
        _ => Uncertainty::Candidate,
    }
}

/// Maps a compilation's own declared `state` (the context envelope's
/// per-compilation field -- `"complete"`/`"partial"`/`"unsupported"`) to the
/// `Incomplete`/`Unsupported` states an occurrence in that compilation
/// inherits, or `None` for `"complete"`, meaning the occurrence's own
/// resolution decides instead.
pub fn from_compilation_state(state: &str, reason: &str) -> Option<Uncertainty> {
    match state {
        "complete" => None,
        "unsupported" => Some(Uncertainty::Unsupported),
        _ => Some(Uncertainty::Incomplete {
            reason: reason.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognised_resolutions_map_to_their_own_state() {
        assert_eq!(from_resolution("confirmed"), Uncertainty::Confirmed);
        assert_eq!(from_resolution("ambiguous"), Uncertainty::Ambiguous);
        assert_eq!(from_resolution("unresolved"), Uncertainty::Unresolved);
        assert_eq!(from_resolution("dynamic"), Uncertainty::Unsupported);
        assert_eq!(from_resolution("inaccessible"), Uncertainty::Excluded);
    }

    #[test]
    fn an_unrecognised_resolution_is_a_candidate_not_a_confirmation() {
        assert_eq!(from_resolution("future-outcome"), Uncertainty::Candidate);
        assert!(!from_resolution("future-outcome").is_confirmed());
    }

    #[test]
    fn only_confirmed_participates_in_override() {
        assert!(Uncertainty::Confirmed.is_confirmed());
        assert!(!Uncertainty::Ambiguous.is_confirmed());
        assert!(!Uncertainty::Unresolved.is_confirmed());
        assert!(!Uncertainty::Unsupported.is_confirmed());
        assert!(!Uncertainty::Excluded.is_confirmed());
        assert!(!Uncertainty::Candidate.is_confirmed());
        assert!(!Uncertainty::Stale {
            reason: "x".to_string()
        }
        .is_confirmed());
        assert!(!Uncertainty::Incomplete {
            reason: "x".to_string()
        }
        .is_confirmed());
    }

    #[test]
    fn a_complete_compilation_defers_to_the_occurrence_resolution() {
        assert_eq!(from_compilation_state("complete", "complete"), None);
    }

    #[test]
    fn a_partial_or_unsupported_compilation_overrides_the_occurrence() {
        assert_eq!(
            from_compilation_state("partial", "binding-error"),
            Some(Uncertainty::Incomplete {
                reason: "binding-error".to_string()
            })
        );
        assert_eq!(
            from_compilation_state("unsupported", "no-target"),
            Some(Uncertainty::Unsupported)
        );
    }

    #[test]
    fn every_state_carries_a_nonempty_reason() {
        let states = [
            Uncertainty::Confirmed,
            Uncertainty::Candidate,
            Uncertainty::Ambiguous,
            Uncertainty::Unresolved,
            Uncertainty::Unsupported,
            Uncertainty::Excluded,
            Uncertainty::Stale {
                reason: "stale-x".to_string(),
            },
            Uncertainty::Incomplete {
                reason: "incomplete-x".to_string(),
            },
        ];
        for state in states {
            assert!(!state.reason().is_empty());
        }
    }
}
