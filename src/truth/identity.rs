//! Symbol and occurrence identity for the semantic-truth harness.
//!
//! A [`SymbolIdentity`] names a member completely enough to survive
//! serialization, deduplication, graph projection and query joins without
//! collapsing two distinct overloads or same-line call sites into one
//! answer. [`OccurrenceSpan`] is a source span, not a bare `(file,
//! startLine)` pair, so two calls on the same line stay distinct. Neither
//! type derives an equality a caller could reach for as a shortcut: a
//! comparator that wants "did this fact match" compares every field it
//! actually cares about, explicitly.

use serde::{Deserialize, Serialize};

/// The compiled project/target-framework/configuration a fact was produced
/// under. Distinguishes the same member built for two different targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompilationIdentity {
    /// The project (csproj/build unit) name.
    pub project: String,
    /// The target framework moniker this compilation was built for.
    pub tfm: String,
    /// The build configuration (`Debug`/`Release`).
    pub configuration: String,
}

/// A complete symbol identity: assembly, declaring type, member, generic
/// arity, overload signature, and the compilation it was produced under.
///
/// Two identities compare equal only when every field matches -- there is
/// no partial-identity shortcut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolIdentity {
    /// The declaring assembly's name.
    pub assembly: String,
    /// The fully qualified declaring type.
    pub declaring_type: String,
    /// The member name.
    pub member: String,
    /// The member's own generic arity (0 for a non-generic member).
    pub generic_arity: u32,
    /// The overload's parameter-shape signature, distinguishing sibling
    /// overloads that share a name and arity.
    pub overload_signature: String,
    /// The compilation this identity was produced under.
    pub compilation: CompilationIdentity,
}

/// A source occurrence, identified by its full span rather than a bare
/// starting line.
///
/// Two calls on one line, or two overloads at different columns of the
/// same line, stay distinct occurrences under this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OccurrenceSpan {
    /// The repository-relative source file.
    pub file: String,
    /// One-based starting line.
    pub start_line: u32,
    /// One-based starting column.
    pub start_col: u32,
    /// One-based ending line.
    pub end_line: u32,
    /// One-based ending column.
    pub end_col: u32,
}

/// The compatibility join key today's shipped `audit.rs` joins facts by:
/// `(file, startLine, member)`, with no column, end span, or compilation
/// identity.
///
/// This type exists only so a comparator can reproduce that legacy join
/// deliberately, as a named limitation -- never as a stand-in for
/// [`SymbolIdentity`]/[`OccurrenceSpan`] equality. It derives neither
/// `PartialEq` nor `Hash`, so nothing can use it as identity by mistake;
/// [`CompatibilityJoinKey::joins`] is the one explicit comparison this type
/// offers.
#[derive(Debug, Clone)]
pub struct CompatibilityJoinKey {
    /// The source file.
    pub file: String,
    /// The starting line the legacy join keys off.
    pub start_line: u32,
    /// The member name.
    pub member: String,
}

impl CompatibilityJoinKey {
    /// Derives the legacy join key from a full identity and occurrence.
    pub fn from_identity(identity: &SymbolIdentity, occurrence: &OccurrenceSpan) -> Self {
        CompatibilityJoinKey {
            file: occurrence.file.clone(),
            start_line: occurrence.start_line,
            member: identity.member.clone(),
        }
    }

    /// Whether two occurrences collapse onto the same legacy join key --
    /// true for two distinct overloads or same-line calls to the same
    /// member, which is exactly the collapse this harness exists to catch.
    pub fn joins(&self, other: &CompatibilityJoinKey) -> bool {
        self.file == other.file
            && self.start_line == other.start_line
            && self.member == other.member
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(member: &str, arity: u32, signature: &str) -> SymbolIdentity {
        SymbolIdentity {
            assembly: "Fixture".to_string(),
            declaring_type: "Fixture.Cases".to_string(),
            member: member.to_string(),
            generic_arity: arity,
            overload_signature: signature.to_string(),
            compilation: CompilationIdentity {
                project: "Cases".to_string(),
                tfm: "net8.0".to_string(),
                configuration: "Release".to_string(),
            },
        }
    }

    fn occurrence(start_col: u32, end_col: u32) -> OccurrenceSpan {
        OccurrenceSpan {
            file: "src/Cases.cs".to_string(),
            start_line: 12,
            start_col,
            end_line: 12,
            end_col,
        }
    }

    #[test]
    fn distinct_overloads_on_one_line_stay_distinct_identities() {
        let int_call = identity("Ping", 0, "Ping(int)");
        let string_call = identity("Ping", 0, "Ping(string)");
        assert_ne!(int_call, string_call);
        assert_ne!(occurrence(5, 12), occurrence(14, 22));
    }

    #[test]
    fn a_symbol_identity_round_trips_through_json_unchanged() {
        let original = identity("Ping", 0, "Ping(int)");
        let json = serde_json::to_string(&original).unwrap();
        let restored: SymbolIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn an_occurrence_span_round_trips_through_json_unchanged() {
        let original = occurrence(5, 12);
        let json = serde_json::to_string(&original).unwrap();
        let restored: OccurrenceSpan = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn compatibility_join_key_collapses_what_full_identity_keeps_apart() {
        let int_call = identity("Ping", 0, "Ping(int)");
        let string_call = identity("Ping", 0, "Ping(string)");
        let a = CompatibilityJoinKey::from_identity(&int_call, &occurrence(5, 12));
        let b = CompatibilityJoinKey::from_identity(&string_call, &occurrence(14, 22));
        assert!(
            int_call != string_call,
            "full identity distinguishes the overloads"
        );
        assert!(
            a.joins(&b),
            "the legacy (file, startLine, member) key does not"
        );
    }
}
