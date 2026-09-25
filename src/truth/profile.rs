//! The compatibility-profile registry.
//!
//! Seven dimensions (host, project/build context, target API contract,
//! language/compiler, application framework, platform/workload,
//! execution context) combined into a named `Profile`. No profile
//! registered here claims `Passing`/`Failing` -- those live only on the
//! per-axis capability matrix, and only once an obligation has actually
//! executed. This registry states the intended inventory and, for the
//! handful of demonstrated targets, that they have been smoke-tested;
//! earning a stronger state for a promised target is a later ticket's
//! job, not this file's.

use super::capability::CapabilityState;

/// One of the seven registered compatibility dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dimension {
    /// The analyzer host (SDK/tool version) a profile runs under.
    Host,
    /// The project/build-context model a profile assumes.
    ProjectBuildContext,
    /// The target API surface a profile is checked against.
    TargetApiContract,
    /// The language/compiler pairing a profile assumes.
    LanguageCompiler,
    /// The application framework a profile's cases target.
    ApplicationFramework,
    /// The platform/workload a profile targets.
    PlatformWorkload,
    /// The execution context (local, CI, scheduled) a profile runs under.
    ExecutionContext,
}

impl Dimension {
    /// Every registered dimension, in declaration order.
    pub const ALL: [Dimension; 7] = [
        Dimension::Host,
        Dimension::ProjectBuildContext,
        Dimension::TargetApiContract,
        Dimension::LanguageCompiler,
        Dimension::ApplicationFramework,
        Dimension::PlatformWorkload,
        Dimension::ExecutionContext,
    ];

    /// The wire label for this dimension.
    pub fn label(self) -> &'static str {
        match self {
            Dimension::Host => "host",
            Dimension::ProjectBuildContext => "project-build-context",
            Dimension::TargetApiContract => "target-api-contract",
            Dimension::LanguageCompiler => "language-compiler",
            Dimension::ApplicationFramework => "application-framework",
            Dimension::PlatformWorkload => "platform-workload",
            Dimension::ExecutionContext => "execution-context",
        }
    }
}

/// A named combination of the seven dimensions, plus the ceiling this
/// registry claims for it today. The ceiling is never `Passing`/`Failing`
/// -- see the module note.
#[derive(Debug, Clone)]
pub struct Profile {
    /// The profile's stable id.
    pub id: &'static str,
    /// The target framework moniker this profile names.
    pub target_framework: &'static str,
    /// The strongest capability state this registry claims for the profile.
    pub ceiling: CapabilityState,
}

/// The full intended inventory: every entry `planned` or `unavailable`
/// except the demonstrated targets, which claim at most `smoke-tested`
/// under the pinned host this registry was probed against.
pub const REGISTERED_PROFILES: &[Profile] = &[
    Profile {
        id: "csharp-net8.0-sdk",
        target_framework: "net8.0",
        ceiling: CapabilityState::SmokeTested,
    },
    Profile {
        id: "csharp-netcoreapp3.1-sdk",
        target_framework: "netcoreapp3.1",
        ceiling: CapabilityState::SmokeTested,
    },
    Profile {
        id: "csharp-net472-framework",
        target_framework: "net472",
        ceiling: CapabilityState::SmokeTested,
    },
    Profile {
        id: "csharp-netstandard2.1-library",
        target_framework: "netstandard2.1",
        ceiling: CapabilityState::SmokeTested,
    },
    Profile {
        id: "csharp-net40-framework",
        target_framework: "net40",
        ceiling: CapabilityState::Planned,
    },
    Profile {
        id: "csharp-net481-framework",
        target_framework: "net481",
        ceiling: CapabilityState::Planned,
    },
    Profile {
        id: "csharp-net10.0-sdk",
        target_framework: "net10.0",
        ceiling: CapabilityState::Planned,
    },
    Profile {
        id: "csharp-netcoreapp2.x-sdk",
        target_framework: "netcoreapp2.x",
        ceiling: CapabilityState::Unavailable,
    },
    Profile {
        id: "csharp-netstandard1.0-library",
        target_framework: "netstandard1.0",
        ceiling: CapabilityState::Planned,
    },
];

/// Looks up a registered profile by id.
pub fn find(id: &str) -> Option<&'static Profile> {
    REGISTERED_PROFILES.iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_registered_profile_claims_passing_or_failing() {
        for profile in REGISTERED_PROFILES {
            assert!(
                !matches!(
                    profile.ceiling,
                    CapabilityState::Passing(_) | CapabilityState::Failing
                ),
                "{} claims a state this registry may not assert",
                profile.id
            );
        }
    }

    #[test]
    fn the_four_demonstrated_targets_are_registered_at_most_smoke_tested() {
        for id in [
            "csharp-net8.0-sdk",
            "csharp-netcoreapp3.1-sdk",
            "csharp-net472-framework",
            "csharp-netstandard2.1-library",
        ] {
            let profile = find(id).unwrap_or_else(|| panic!("{id} must be registered"));
            assert_eq!(profile.ceiling, CapabilityState::SmokeTested);
        }
    }

    #[test]
    fn lookup_of_an_unregistered_id_is_none() {
        assert!(find("csharp-vb-anything").is_none());
    }
}
