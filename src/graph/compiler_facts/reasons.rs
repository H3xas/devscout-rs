// The closed refusal vocabulary `admit` and `run` return. Every variant
// writes a stable, machine-readable token (never a formatted sentence) so a
// caller can match on the string alone; a human-readable detail, when one
// exists, is carried separately by the caller rather than folded into the
// token itself. Two disjoint families: identity (a mismatch between the
// candidate and what this checkout expects) and broken-run (a candidate that
// cannot be trusted at all, whether killed before it produced one or
// self-contradictory once parsed). A structurally valid artifact that
// declares incomplete coverage is admitted, never refused -- see
// `super::admit::Coverage`.

use std::fmt;

/// Why a candidate compiler-facts artifact was refused.
///
/// `Display` writes the exact token named in each variant's doc comment;
/// nothing here is reworded once shipped, since callers match on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalReason {
    // --- broken-run family: the candidate cannot be trusted at all -----
    /// `engine-killed`: the local engine process exited with a non-zero or
    /// signalled status, or could not be started at all.
    EngineKilled,
    /// `engine-timeout`: the local engine process did not finish inside the
    /// configured wall-clock budget and was killed.
    EngineTimeout,
    /// `output-bounded`: the local engine process wrote more than the
    /// configured output cap and was killed before finishing.
    OutputBounded,
    /// `malformed-encoding`: the candidate bytes do not parse as the
    /// documented JSON envelope shape (invalid JSON, a non-object root, an
    /// unrecognised `format`, or a header field of the wrong type).
    MalformedEncoding,
    /// `missing-completion-record`: the candidate has no explicit terminal
    /// completion record, so a killed or truncated run cannot be told apart
    /// from a genuinely finished one by parse success alone.
    MissingCompletionRecord,
    /// `incoherent-inventory`: the candidate's own declared per-unit
    /// inventory contradicts itself (a unit id claimed both processed and
    /// missing).
    IncoherentInventory,

    // --- identity family: a well-formed candidate that does not match ---
    /// `engine-revision-mismatch`: the candidate's producer engine revision
    /// does not match the revision this checkout's admission path expects.
    EngineRevisionMismatch,
    /// `contract-version-mismatch`: the candidate's wire-contract version
    /// does not match the version this admission path was built for.
    ContractVersionMismatch,
    /// `profile-mismatch`: the candidate's requested compilation profile
    /// (target, configuration, platform) does not match the profile that
    /// was requested of it.
    ProfileMismatch,
    /// `dependency-fingerprint-mismatch`: the candidate's engine-dependency
    /// fingerprint does not match the fingerprint this admission path
    /// expects of the shipped engine build.
    DependencyFingerprintMismatch,
    /// `context-envelope-version-unrecognised`: the candidate embeds a
    /// compilation-context envelope version this admission path does not
    /// recognise.
    ContextEnvelopeVersionUnrecognised,
    /// `context-fingerprint-mismatch`: the candidate's header-level context
    /// fingerprint summary does not match the fingerprint carried inside
    /// its own embedded context envelope -- a self-consistency check,
    /// distinct from `DependencyFingerprintMismatch`, which is about the
    /// engine's own build rather than the compilation it analysed.
    ContextFingerprintMismatch,
    /// `source-snapshot-mismatch`: the candidate's declared source-snapshot
    /// identity (`headSha`) does not match this checkout's own HEAD.
    SourceSnapshotMismatch,
}

impl RefusalReason {
    /// The stable machine-readable token, as an existing precedent expects:
    /// see `graph::imports::parse_imported_edges`'s own "message naming the
    /// offending value" convention, applied here as a fixed token first.
    pub const fn token(self) -> &'static str {
        match self {
            RefusalReason::EngineKilled => "engine-killed",
            RefusalReason::EngineTimeout => "engine-timeout",
            RefusalReason::OutputBounded => "output-bounded",
            RefusalReason::MalformedEncoding => "malformed-encoding",
            RefusalReason::MissingCompletionRecord => "missing-completion-record",
            RefusalReason::IncoherentInventory => "incoherent-inventory",
            RefusalReason::EngineRevisionMismatch => "engine-revision-mismatch",
            RefusalReason::ContractVersionMismatch => "contract-version-mismatch",
            RefusalReason::ProfileMismatch => "profile-mismatch",
            RefusalReason::DependencyFingerprintMismatch => "dependency-fingerprint-mismatch",
            RefusalReason::ContextEnvelopeVersionUnrecognised => {
                "context-envelope-version-unrecognised"
            }
            RefusalReason::ContextFingerprintMismatch => "context-fingerprint-mismatch",
            RefusalReason::SourceSnapshotMismatch => "source-snapshot-mismatch",
        }
    }
}

impl fmt::Display for RefusalReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.token())
    }
}
