//! The independent semantic-truth, profile, and falsification harness.
//!
//! This module grades a semantic analyzer against reviewed, versioned
//! expectations rather than the analyzer's own output: an empty or
//! partial result can never pass, a fault-control battery proves every
//! registered wrong-system mutation is actually caught, a profile
//! registry and capability matrix report every requested target and every
//! unexecuted combination, and a committed red baseline records today's
//! known misses as explicit, attributed rows rather than repairing them
//! in place. `fixtures/csharp-truth/` is the checked-in case manifest and
//! fixture pack this module's tests grade against; `tests/semantic_truth_
//! *.rs` map one file to each area of acceptance evidence.

/// The machine-readable capability matrix: state per (profile, axis).
pub mod capability;
/// The registered fault-control battery and its exhaustiveness check.
pub mod fault_controls;
/// Freshness and transformation controls, run in both directions.
pub mod freshness;
/// Symbol and occurrence identity, plus the legacy compatibility join key.
pub mod identity;
/// The case-manifest schema, its validator, and its parser.
pub mod manifest;
/// The pinned foundation-candidate digest and its offline verification.
pub mod pin;
/// Readers that turn committed producer bytes into observations.
///
/// Used instead of hand-building an observation from a manifest's own
/// expectations.
pub mod producer_reader;
/// The compatibility-profile registry.
pub mod profile;
/// The committed truthful red baseline.
pub mod red_baseline;
/// Case evaluation and the two-lane `TruthReport` artifact.
pub mod report;
/// The uncertainty and context-health vocabulary.
pub mod uncertainty;
