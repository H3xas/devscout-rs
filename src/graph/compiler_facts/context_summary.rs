// The derived context-summary fold: recomputes the header's own
// `context.contextFingerprint` from the embedded envelope's ordered
// per-compilation `identity`+`fingerprint` pairs, replacing the old
// self-referential literal-equals-itself check with a comparison against a
// real recomputation. Pure, no I/O. Independently reimplemented -- not
// shared code -- on the C# producer side
// (tools/scout-semantic/CompilerOccurrences.cs's `DerivedContextSummary`),
// so the two sides must agree byte-for-byte on the same fold rule rather
// than call into one another.

use sha2::{Digest, Sha256};

use super::artifact::CompilationRef;

/// Stands in for a JSON `null` fingerprint (an envelope's own "unsupported
/// compilation" case). Four bytes; a real fingerprint is 40 lowercase hex
/// characters, so this can never collide with one.
const NULL_FINGERPRINT_SENTINEL: &str = "null";

/// Folds `compilations` -- in the order given, never re-sorted here -- into
/// a lower-case hex SHA-256 digest of their canonicalized `identity`+
/// `fingerprint` pairs, one pre-image line per compilation joined with
/// `\n`. Each line is `<canonical identity JSON>\x1f<fingerprint-or-"null">`
/// (`\x1f`, ASCII unit separator, cannot appear inside compact JSON text).
///
/// `identity` is canonicalized by `serde_json::to_string`: this crate's
/// `serde_json::Map` defaults to a `BTreeMap` (the `preserve_order` feature
/// is not enabled in `Cargo.toml`), so object keys already serialize sorted
/// ordinally at every nesting level and arrays keep their given order --
/// exactly the canonicalization rule the contract states, for free.
pub fn recompute(compilations: &[CompilationRef]) -> String {
    let mut lines = Vec::with_capacity(compilations.len());
    for compilation in compilations {
        let canonical =
            serde_json::to_string(&compilation.identity).unwrap_or_else(|_| "null".to_string());
        let fingerprint = compilation
            .fingerprint
            .as_deref()
            .unwrap_or(NULL_FINGERPRINT_SENTINEL);
        lines.push(format!("{canonical}\u{1f}{fingerprint}"));
    }

    let preimage = lines.join("\n");
    let digest = Sha256::digest(preimage.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }

    hex
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn empty_compilation_list_folds_to_a_stable_digest() {
        let digest = recompute(&[]);
        assert_eq!(digest.len(), 64);
        // The empty preimage's own SHA-256 digest, pinned so a future change
        // to this fold is a visible, deliberate diff rather than a silent
        // drift.
        assert_eq!(
            digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn key_order_in_the_source_identity_value_does_not_move_the_digest() {
        let a = CompilationRef {
            identity: json!({"projectPath": "P.csproj", "projectName": "P", "requestedTfm": "net9.0"}),
            fingerprint: Some("a".repeat(40)),
        };
        let b = CompilationRef {
            identity: json!({"requestedTfm": "net9.0", "projectName": "P", "projectPath": "P.csproj"}),
            fingerprint: Some("a".repeat(40)),
        };
        assert_eq!(recompute(std::slice::from_ref(&a)), recompute(&[b]));
    }

    #[test]
    fn a_null_fingerprint_uses_the_sentinel_and_cannot_collide_with_a_real_one() {
        let unsupported = CompilationRef {
            identity: json!({"projectPath": "P.csproj"}),
            fingerprint: None,
        };
        let literal_null_string = CompilationRef {
            identity: json!({"projectPath": "P.csproj"}),
            fingerprint: Some("null".to_string()),
        };
        assert_eq!(
            recompute(&[unsupported]),
            recompute(&[literal_null_string]),
            "the sentinel and a real (if implausible) 4-char fingerprint string collide only \
             because a genuine fingerprint is always 40 hex characters, never 4"
        );
    }

    #[test]
    fn two_compilations_fold_differently_from_one_with_the_same_total_content() {
        let one = CompilationRef {
            identity: json!({"projectPath": "P.csproj"}),
            fingerprint: Some("a".repeat(40)),
        };
        let two = [
            CompilationRef {
                identity: json!({"projectPath": "P.csproj"}),
                fingerprint: Some("a".repeat(20)),
            },
            CompilationRef {
                identity: json!({}),
                fingerprint: Some("a".repeat(20)),
            },
        ];
        assert_ne!(recompute(std::slice::from_ref(&one)), recompute(&two));
    }
}

