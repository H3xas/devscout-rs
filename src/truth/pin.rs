//! The pinned foundation-candidate identity.
//!
//! A content digest over the manifest and fixture pack bytes, plus the
//! contract version, that a consumer verifies offline. [`verify_pin`] has
//! no mutating counterpart in this module -- a disagreeing consumer can
//! only report a [`Disagreement`], never amend, back-fill, or re-derive
//! the pinned expectation.

use sha2::{Digest, Sha256};
use std::fmt::Write as _;

/// The pinned identity a consumer checks its own copy against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    /// The contract version the digest was computed under.
    pub contract: String,
    /// The lowercase hex content digest.
    pub digest_hex: String,
}

/// Computes the pin over the manifest and fixture-source bytes and the
/// contract version.
///
/// Byte-order of the inputs is fixed by the caller (manifest first, then
/// each fixture file in a stable, sorted order), so the same inputs
/// always produce the same digest.
pub fn compute_pin(contract: &str, manifest_bytes: &[u8], fixture_files: &[(&str, &[u8])]) -> Pin {
    let mut hasher = Sha256::new();
    hasher.update(contract.as_bytes());
    hasher.update(b"\0");
    hasher.update(manifest_bytes);
    let mut sorted: Vec<&(&str, &[u8])> = fixture_files.iter().collect();
    sorted.sort_by_key(|(path, _)| *path);
    for (path, bytes) in sorted {
        hasher.update(b"\0");
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(bytes);
    }
    let digest = hasher.finalize();
    let mut digest_hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(digest_hex, "{byte:02x}");
    }
    Pin {
        contract: contract.to_string(),
        digest_hex,
    }
}

/// A consumer's own copy disagreed with the pin. This is the only outcome
/// a disagreeing consumer can reach -- there is no path anywhere in this
/// module from a `Disagreement` back into a rewritten `Pin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disagreement {
    /// The pin the consumer's copy was checked against.
    pub expected: Pin,
    /// The digest the consumer actually computed.
    pub observed_digest_hex: String,
}

/// Confirms a consumer's own recomputed digest matches the pin, offline.
/// Returns the mismatch as data on disagreement; nothing here can write a
/// new pin.
pub fn verify_pin(expected: &Pin, observed_digest_hex: &str) -> Result<(), Disagreement> {
    if expected.digest_hex == observed_digest_hex {
        Ok(())
    } else {
        Err(Disagreement {
            expected: expected.clone(),
            observed_digest_hex: observed_digest_hex.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_inputs_produce_the_same_pin() {
        let a = compute_pin("semantic-truth-v1", b"{}", &[("src/A.cs", b"class A {}")]);
        let b = compute_pin("semantic-truth-v1", b"{}", &[("src/A.cs", b"class A {}")]);
        assert_eq!(a, b);
    }

    #[test]
    fn fixture_file_order_does_not_change_the_pin() {
        let a = compute_pin(
            "semantic-truth-v1",
            b"{}",
            &[("src/A.cs", b"class A {}"), ("src/B.cs", b"class B {}")],
        );
        let b = compute_pin(
            "semantic-truth-v1",
            b"{}",
            &[("src/B.cs", b"class B {}"), ("src/A.cs", b"class A {}")],
        );
        assert_eq!(
            a, b,
            "the digest sorts fixture files itself, independent of call order"
        );
    }

    #[test]
    fn a_disagreeing_consumer_output_is_reported_never_applied() {
        let pin = compute_pin("semantic-truth-v1", b"{}", &[]);
        let result = verify_pin(&pin, "not-the-real-digest");
        assert_eq!(
            result,
            Err(Disagreement {
                expected: pin,
                observed_digest_hex: "not-the-real-digest".to_string(),
            })
        );
    }

    #[test]
    fn a_matching_consumer_output_verifies() {
        let pin = compute_pin("semantic-truth-v1", b"{}", &[]);
        assert_eq!(verify_pin(&pin, &pin.digest_hex), Ok(()));
    }
}
