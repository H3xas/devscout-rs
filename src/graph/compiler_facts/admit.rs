// The pure admission core: no I/O, no process, no filesystem access. Every
// refusal test in this crate calls this function directly or indirectly
// through the CLI, so its check order is fixed and documented here rather
// than left to fall out of the code -- a predictable "first failing check
// wins" story every refusal test asserts against.

use super::artifact::{parse_header, CandidateHeader, Coverage};
use super::expectations::AdmissionExpectations;
use super::reasons::RefusalReason;

/// A candidate that passed every admission check, still carrying its
/// original bytes unchanged -- the one field publication ever writes.
#[derive(Debug, Clone)]
pub struct AdmittedFacts {
    /// The parsed header.
    pub header: CandidateHeader,
    /// The original candidate bytes, verbatim. Never re-serialized: this is
    /// what makes a lossless round trip possible without this module
    /// modeling the bulk payload at all.
    pub bytes: Vec<u8>,
}

// A unit id claimed both processed and missing is the artifact
// contradicting its own bookkeeping -- the shape of internal incoherence
// this crate can detect without engine cooperation.
fn inventory_is_coherent(header: &CandidateHeader) -> bool {
    !header
        .units_processed
        .iter()
        .any(|unit| header.units_missing.contains(unit))
}

fn identity_check(
    header: &CandidateHeader,
    expected: &AdmissionExpectations,
) -> Option<RefusalReason> {
    if header.contract_version != expected.contract_version {
        return Some(RefusalReason::ContractVersionMismatch);
    }
    if header.engine_revision != expected.engine_revision {
        return Some(RefusalReason::EngineRevisionMismatch);
    }
    if header.profile != expected.profile {
        return Some(RefusalReason::ProfileMismatch);
    }
    if header.dependency_fingerprint != expected.dependency_fingerprint {
        return Some(RefusalReason::DependencyFingerprintMismatch);
    }
    if header.context_schema_version != expected.context_schema_version {
        return Some(RefusalReason::ContextEnvelopeVersionUnrecognised);
    }
    if let (Some(header_fp), Some(envelope_fp)) = (
        &header.context_fingerprint_header,
        &header.context_fingerprint_envelope,
    ) {
        if header_fp != envelope_fp {
            return Some(RefusalReason::ContextFingerprintMismatch);
        }
    }
    if let Some(expected_head) = &expected.head_sha {
        if header.source_head_sha.as_deref() != Some(expected_head.as_str()) {
            return Some(RefusalReason::SourceSnapshotMismatch);
        }
    }
    None
}

/// Admits or refuses a candidate compiler-facts artifact.
///
/// Check order, fixed: malformed encoding, then the terminal completion
/// record, then per-unit inventory coherence, then contract version,
/// engine revision, profile, dependency fingerprint, the embedded
/// context-envelope version, the context fingerprint, and the source
/// snapshot -- and only once every one of those has passed does a
/// structurally valid, declared-incomplete artifact still succeed,
/// carrying its own incomplete [`Coverage`] rather than being refused.
pub fn admit(
    candidate_bytes: &[u8],
    expected: &AdmissionExpectations,
) -> Result<AdmittedFacts, RefusalReason> {
    let header = parse_header(candidate_bytes)?;

    if !header.completion_terminal {
        return Err(RefusalReason::MissingCompletionRecord);
    }
    if !inventory_is_coherent(&header) {
        return Err(RefusalReason::IncoherentInventory);
    }
    if let Some(reason) = identity_check(&header, expected) {
        return Err(reason);
    }

    Ok(AdmittedFacts {
        header,
        bytes: candidate_bytes.to_vec(),
    })
}

impl AdmittedFacts {
    /// The admitted artifact's own coverage state.
    pub fn coverage(&self) -> &Coverage {
        &self.header.coverage
    }
}
