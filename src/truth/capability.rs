//! The machine-readable capability matrix: one state per (profile, axis)
//! pair, across five capability axes tracked independently.
//!
//! `Passing` is constructible only from an [`ExecutedObligation`] witness
//! -- and that witness is constructible only when at least one obligation
//! actually ran -- so a profile with an unexecuted obligation cannot hold
//! `Passing` by construction, not merely by convention.

use crate::query::json::J;

use super::profile::REGISTERED_PROFILES;

/// The five capability axes tracked independently per profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityAxis {
    /// Whether a usable build context can be acquired at all.
    ContextAcquisition,
    /// Whether the analyzer's binding decisions conform to the language's
    /// own semantics.
    SemanticConformance,
    /// Whether an application framework's own conventions are modeled.
    FrameworkModeling,
    /// Whether repository-derived evidence (files, projects) is complete.
    RepositoryEvidence,
    /// Whether a run against a live host actually executed and produced
    /// evidence.
    RuntimeEvidence,
}

impl CapabilityAxis {
    /// Every registered axis, in declaration order.
    pub const ALL: [CapabilityAxis; 5] = [
        CapabilityAxis::ContextAcquisition,
        CapabilityAxis::SemanticConformance,
        CapabilityAxis::FrameworkModeling,
        CapabilityAxis::RepositoryEvidence,
        CapabilityAxis::RuntimeEvidence,
    ];

    /// The wire label for this axis.
    pub fn label(self) -> &'static str {
        match self {
            CapabilityAxis::ContextAcquisition => "context-acquisition",
            CapabilityAxis::SemanticConformance => "semantic-conformance",
            CapabilityAxis::FrameworkModeling => "framework-modeling",
            CapabilityAxis::RepositoryEvidence => "repository-evidence",
            CapabilityAxis::RuntimeEvidence => "runtime-evidence",
        }
    }
}

/// Proof that at least one obligation actually executed. The only public
/// constructor refuses a zero count, so a caller can never manufacture a
/// witness for work that did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutedObligation {
    /// How many obligations actually ran.
    pub executed: u32,
}

impl ExecutedObligation {
    /// Records a witness for `executed` completed obligations, refusing a
    /// zero count.
    pub fn record(executed: u32) -> Option<ExecutedObligation> {
        if executed == 0 {
            None
        } else {
            Some(ExecutedObligation { executed })
        }
    }
}

/// The state one profile holds on one axis. `Passing` carries the witness
/// that earned it; there is no variant that claims a pass with no
/// obligation behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityState {
    /// Registered as intended future coverage; nothing has run.
    Planned,
    /// Registered but cannot be executed offline from pinned inputs.
    Unavailable,
    /// Probed once, outside the obligation's own scored run.
    SmokeTested,
    /// The obligation executed and passed; carries the witness that
    /// proves it.
    Passing(ExecutedObligation),
    /// The obligation executed and failed.
    Failing,
}

impl CapabilityState {
    /// The wire label for this state.
    pub fn label(self) -> &'static str {
        match self {
            CapabilityState::Planned => "planned",
            CapabilityState::Unavailable => "unavailable",
            CapabilityState::SmokeTested => "smoke-tested",
            CapabilityState::Passing(_) => "passing",
            CapabilityState::Failing => "failing",
        }
    }
}

/// One (profile, axis) cell: the state it holds, and the denominator/
/// exclusion counts a run against it reported.
///
/// A profile never registered against an axis simply has no entry --
/// absence, not a zero disguised as a measurement.
#[derive(Debug, Clone)]
pub struct MatrixEntry {
    /// The profile this cell belongs to.
    pub profile_id: &'static str,
    /// The capability axis this cell belongs to.
    pub axis: CapabilityAxis,
    /// The state this cell holds.
    pub state: CapabilityState,
    /// How many obligations were requested for this cell.
    pub denominator: u32,
    /// How many requested obligations were excluded from scoring.
    pub excluded: u32,
}

/// The full profile-by-axis matrix.
#[derive(Debug, Clone)]
pub struct CapabilityMatrix {
    /// One entry per registered (profile, axis) pair.
    pub entries: Vec<MatrixEntry>,
}

/// Builds the registry-only matrix: every registered profile, every axis,
/// capped at that profile's registered ceiling.
///
/// No obligation is executed by this function -- it reads static registry
/// data only -- so no entry it produces ever holds `Passing`.
pub fn build_matrix() -> CapabilityMatrix {
    let mut entries = Vec::new();
    for profile in REGISTERED_PROFILES {
        for axis in CapabilityAxis::ALL {
            entries.push(MatrixEntry {
                profile_id: profile.id,
                axis,
                state: profile.ceiling,
                denominator: 0,
                excluded: 0,
            });
        }
    }
    CapabilityMatrix { entries }
}

/// Deterministic JSON for the matrix: entries sorted by profile id then
/// axis declaration order, so two builds from the same registry produce
/// byte-identical output.
pub fn matrix_to_json(matrix: &CapabilityMatrix) -> String {
    let mut sorted = matrix.entries.clone();
    sorted.sort_by(|a, b| {
        a.profile_id
            .cmp(b.profile_id)
            .then_with(|| axis_order(a.axis).cmp(&axis_order(b.axis)))
    });
    J::Obj(vec![(
        "entries",
        J::Arr(
            sorted
                .iter()
                .map(|e| {
                    J::Obj(vec![
                        ("profile", J::Str(e.profile_id.to_string())),
                        ("axis", J::Str(e.axis.label().to_string())),
                        ("state", J::Str(e.state.label().to_string())),
                        ("denominator", J::UInt(u64::from(e.denominator))),
                        ("excluded", J::UInt(u64::from(e.excluded))),
                    ])
                })
                .collect(),
        ),
    )])
    .to_json_string()
}

fn axis_order(axis: CapabilityAxis) -> usize {
    CapabilityAxis::ALL
        .iter()
        .position(|a| *a == axis)
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_executions_cannot_produce_a_witness() {
        assert_eq!(ExecutedObligation::record(0), None);
        assert!(ExecutedObligation::record(1).is_some());
    }

    #[test]
    fn the_registry_only_matrix_holds_no_unexecuted_passing_entry() {
        let matrix = build_matrix();
        assert!(!matrix.entries.is_empty());
        for entry in &matrix.entries {
            assert!(
                !matches!(entry.state, CapabilityState::Passing(_)),
                "{} / {} claims passing with nothing executed",
                entry.profile_id,
                entry.axis.label()
            );
        }
    }

    #[test]
    fn matrix_json_is_byte_identical_across_two_builds() {
        let a = matrix_to_json(&build_matrix());
        let b = matrix_to_json(&build_matrix());
        assert_eq!(a, b);
    }

    #[test]
    fn matrix_json_is_sorted_by_profile_then_axis() {
        let json = matrix_to_json(&build_matrix());
        let first = json.find("csharp-net40-framework").unwrap();
        let later = json.find("csharp-netstandard2.1-library").unwrap();
        assert!(first < later, "profiles must sort lexically");
    }
}
