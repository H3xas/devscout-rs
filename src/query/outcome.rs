// The closed `outcome` vocabulary every `--json` answer on `refs`/`read`/
// `impact`/`tests` carries. Defined once, here, as a Rust enum rather than a
// bag of string literals scattered across `cli.rs`'s JSON builders: an
// `Outcome` value can only ever print one of the four words below, so no
// caller can typo a fifth one into existence.

/// The version of the `--json` answer shape every `refs`/`read`/`impact`/
/// `tests` object carries as its first key. Bump it only when an existing key
/// is renamed, removed, or given a different meaning -- adding a new key, as
/// `schema_version` and `why` themselves were added, never requires a bump. A
/// consumer should ignore any key it does not recognise.
pub const SCHEMA_VERSION: u64 = 1;

/// One of the four words a query verb's `--json` answer names its result
/// with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The seed resolved (a type, a member, or a file) and the answer is the
    /// ordinary shape a caller reads.
    Hit,
    /// The seed resolved but the answer itself is empty: `impact` with no affected file,
    /// or `refs`/`read` on a member declared by exactly one type with nothing referencing it.
    ZeroHit,
    /// The seed named more than one candidate (a type, or a member across
    /// more than one declaring type) and nothing was guessed between them.
    Ambiguous,
    /// Nothing in the graph carries the seed at all; the caller's zero-hit
    /// note advises a text-search fallback.
    FallbackAdvised,
}

impl Outcome {
    /// Every value, in the order the doc comment above lists them.
    pub const ALL: [Outcome; 4] = [
        Outcome::Hit,
        Outcome::ZeroHit,
        Outcome::Ambiguous,
        Outcome::FallbackAdvised,
    ];

    /// The exact `--json` word for this outcome.
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Hit => "hit",
            Outcome::ZeroHit => "zero-hit",
            Outcome::Ambiguous => "ambiguous",
            Outcome::FallbackAdvised => "fallback-advised",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_outcome_word_is_one_of_the_closed_vocabulary() {
        const EXPECTED: &[&str] = &["hit", "zero-hit", "ambiguous", "fallback-advised"];
        let words: Vec<&str> = Outcome::ALL.iter().map(|o| o.as_str()).collect();
        for word in &words {
            assert!(
                EXPECTED.contains(word),
                "{word} is not in the closed outcome vocabulary"
            );
        }
        assert_eq!(
            words.len(),
            EXPECTED.len(),
            "ALL must name every vocabulary word exactly once"
        );
    }
}
