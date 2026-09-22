//! The uncertainty and context-health vocabulary shared with the
//! enrichment consumer: eight fact states and five context-health states.
//!
//! Every non-`Confirmed`/non-`Complete` variant carries its reason as a
//! required field, not an optional afterthought -- there is no way to
//! construct e.g. `Stale` without saying why, so a state can never be
//! inferred from an empty or null field.

/// A fact's confidence state. `Confirmed` needs no reason; every other
/// variant is only constructible with one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Uncertainty {
    /// The fact is verified; no reason applies.
    Confirmed,
    /// A plausible but unverified fact.
    Candidate {
        /// Why this fact could not be confirmed.
        reason: String,
    },
    /// More than one candidate target satisfies the site.
    Ambiguous {
        /// Which candidates conflict and why.
        reason: String,
    },
    /// No candidate could be found.
    Unresolved {
        /// Why nothing resolved.
        reason: String,
    },
    /// The construct is outside this harness's supported surface.
    Unsupported {
        /// What is unsupported.
        reason: String,
    },
    /// The fact was deliberately left out of scope.
    Excluded {
        /// Why it was excluded.
        reason: String,
    },
    /// The fact was produced from an input that has since changed.
    Stale {
        /// What changed underneath it.
        reason: String,
    },
    /// The fact is only partially determined.
    Incomplete {
        /// What is missing.
        reason: String,
    },
}

impl Uncertainty {
    /// The manifest/report wire label for this state, matching the
    /// enrichment consumer's own vocabulary spelling.
    pub fn label(&self) -> &'static str {
        match self {
            Uncertainty::Confirmed => "confirmed",
            Uncertainty::Candidate { .. } => "candidate",
            Uncertainty::Ambiguous { .. } => "ambiguous",
            Uncertainty::Unresolved { .. } => "unresolved",
            Uncertainty::Unsupported { .. } => "unsupported",
            Uncertainty::Excluded { .. } => "excluded",
            Uncertainty::Stale { .. } => "stale",
            Uncertainty::Incomplete { .. } => "incomplete",
        }
    }

    /// The required reason for every state but `Confirmed`.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Uncertainty::Confirmed => None,
            Uncertainty::Candidate { reason }
            | Uncertainty::Ambiguous { reason }
            | Uncertainty::Unresolved { reason }
            | Uncertainty::Unsupported { reason }
            | Uncertainty::Excluded { reason }
            | Uncertainty::Stale { reason }
            | Uncertainty::Incomplete { reason } => Some(reason),
        }
    }

    /// Builds a state from its wire label and an optional reason, refusing
    /// a non-`confirmed` label with no reason and an unknown label
    /// outright -- the one place a manifest/report string becomes this
    /// type.
    pub fn from_label(label: &str, reason: Option<String>) -> Result<Uncertainty, String> {
        match label {
            "confirmed" => Ok(Uncertainty::Confirmed),
            "candidate" => require_reason(reason).map(|reason| Uncertainty::Candidate { reason }),
            "ambiguous" => require_reason(reason).map(|reason| Uncertainty::Ambiguous { reason }),
            "unresolved" => require_reason(reason).map(|reason| Uncertainty::Unresolved { reason }),
            "unsupported" => {
                require_reason(reason).map(|reason| Uncertainty::Unsupported { reason })
            }
            "excluded" => require_reason(reason).map(|reason| Uncertainty::Excluded { reason }),
            "stale" => require_reason(reason).map(|reason| Uncertainty::Stale { reason }),
            "incomplete" => require_reason(reason).map(|reason| Uncertainty::Incomplete { reason }),
            other => Err(format!("unknown uncertainty state '{other}'")),
        }
    }
}

fn require_reason(reason: Option<String>) -> Result<String, String> {
    match reason {
        Some(reason) if !reason.trim().is_empty() => Ok(reason),
        _ => Err("a non-confirmed uncertainty state requires a reason".to_string()),
    }
}

/// The build-context health an analyzer run reports. `Complete` needs no
/// reason; every other variant does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextHealth {
    /// Every requested project/document resolved and built cleanly.
    Complete,
    /// Some requested project/document is missing from the run.
    Partial {
        /// What is missing.
        reason: String,
    },
    /// The run could not produce a usable context at all.
    Failed {
        /// Why the run failed.
        reason: String,
    },
    /// The requested context is outside this harness's supported surface.
    Unsupported {
        /// What is unsupported.
        reason: String,
    },
    /// The context was deliberately left out of scope.
    Excluded {
        /// Why it was excluded.
        reason: String,
    },
}

impl ContextHealth {
    /// The wire label for this state.
    pub fn label(&self) -> &'static str {
        match self {
            ContextHealth::Complete => "complete",
            ContextHealth::Partial { .. } => "partial",
            ContextHealth::Failed { .. } => "failed",
            ContextHealth::Unsupported { .. } => "unsupported",
            ContextHealth::Excluded { .. } => "excluded",
        }
    }

    /// Whether this run can ever be reported `complete` -- the one
    /// question a positive obligation is allowed to ask before it fails a
    /// failed-or-partial context outright.
    pub fn is_complete(&self) -> bool {
        matches!(self, ContextHealth::Complete)
    }

    /// Builds a state from its wire label and an optional reason, the same
    /// way [`Uncertainty::from_label`] does for the fact vocabulary.
    pub fn from_label(label: &str, reason: Option<String>) -> Result<ContextHealth, String> {
        match label {
            "complete" => Ok(ContextHealth::Complete),
            "partial" => require_reason(reason).map(|reason| ContextHealth::Partial { reason }),
            "failed" => require_reason(reason).map(|reason| ContextHealth::Failed { reason }),
            "unsupported" => {
                require_reason(reason).map(|reason| ContextHealth::Unsupported { reason })
            }
            "excluded" => require_reason(reason).map(|reason| ContextHealth::Excluded { reason }),
            other => Err(format!("unknown context health '{other}'")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_needs_no_reason_but_every_other_state_does() {
        assert_eq!(
            Uncertainty::from_label("confirmed", None),
            Ok(Uncertainty::Confirmed)
        );
        assert!(Uncertainty::from_label("stale", None).is_err());
        assert_eq!(
            Uncertainty::from_label("stale", Some("dependency compilation changed".to_string())),
            Ok(Uncertainty::Stale {
                reason: "dependency compilation changed".to_string()
            })
        );
    }

    #[test]
    fn a_failed_or_partial_context_is_never_complete() {
        let partial =
            ContextHealth::from_label("partial", Some("one project missing".to_string())).unwrap();
        assert!(!partial.is_complete());
        assert!(ContextHealth::Complete.is_complete());
    }

    #[test]
    fn unknown_label_is_refused() {
        assert!(Uncertainty::from_label("bogus", None).is_err());
        assert!(ContextHealth::from_label("bogus", None).is_err());
    }
}
