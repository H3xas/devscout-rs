//! The export's closed evidence vocabulary and the mapping onto it.
//!
//! Every fact line carries exactly one word from [`VOCABULARY`], derived only
//! from fields the index already records. The mapping is written as one match
//! per `Edge` variant with no wildcard arm, so a new variant fails to compile
//! until it is mapped rather than silently exporting as something weaker than
//! it is. The words themselves are the contract's, not a reader's: a consumer
//! that only understands the words this release ships can still refuse the
//! ones it does not.

use crate::graph::Edge;

use crate::graph::HeuristicTier;
use crate::graph::SemanticProvenance;

/// The whole closed vocabulary, in the order the schema lists it. Read by the
/// unit tests, which hold the mapping and the schema's enum to it.
#[cfg(test)]
pub(crate) const VOCABULARY: [&str; 11] = [
    "exact",
    "semantic",
    "semantic-discovered",
    "heuristic-ext",
    "heuristic-guess",
    "heuristic",
    "ambiguous",
    "unresolved",
    "declared",
    "derived",
    "convention",
];

/// A reference the resolver bound to one candidate under a rule it can justify.
pub(crate) const EXACT: &str = "exact";
/// A reference a same-context compiler fact confirmed.
pub(crate) const SEMANTIC: &str = "semantic";
/// A reference only an admitted compiler fact named.
pub(crate) const SEMANTIC_DISCOVERED: &str = "semantic-discovered";
/// A reference the extension-method tier guessed.
pub(crate) const HEURISTIC_EXT: &str = "heuristic-ext";
/// A reference the scored tier guessed by name.
pub(crate) const HEURISTIC_GUESS: &str = "heuristic-guess";
/// A reference an unclassified guess emitted.
pub(crate) const HEURISTIC: &str = "heuristic";
/// A reference the resolver refused to bind: candidates were, or might have
/// been, more than one.
pub(crate) const AMBIGUOUS: &str = "ambiguous";
/// A reference the export knows about but no rule resolved.
pub(crate) const UNRESOLVED: &str = "unresolved";
/// A fact read off the source text as written, not resolved against it.
pub(crate) const DECLARED: &str = "declared";
/// A fact derived from two recorded facts rather than read off either.
pub(crate) const DERIVED: &str = "derived";
/// A fact this project's own naming convention puts on the file, with nothing
/// in the source saying so.
pub(crate) const CONVENTION: &str = "convention";

/// The evidence word for one graph edge.
pub(crate) fn edge_evidence(edge: &Edge) -> &'static str {
    match edge {
        // A flagged inherits/uses-type edge is a guess the resolver could not
        // attribute to a tier: the flag predates the tier split, so the export
        // says only "guessed".
        Edge::Inherits { heuristic, .. } | Edge::UsesType { heuristic, .. } => {
            if *heuristic {
                HEURISTIC
            } else {
                EXACT
            }
        }
        Edge::UsesMember {
            heuristic,
            tier,
            source,
            ..
        } => match (source, tier, heuristic) {
            (Some(SemanticProvenance::Semantic), _, _) => SEMANTIC,
            (Some(SemanticProvenance::SemanticDiscovered), _, _) => SEMANTIC_DISCOVERED,
            (None, Some(HeuristicTier::Ext), _) => HEURISTIC_EXT,
            (None, Some(HeuristicTier::Guess), _) => HEURISTIC_GUESS,
            (None, None, true) => HEURISTIC,
            (None, None, false) => EXACT,
        },
        // A C# `using` names a namespace and resolves to no file. It is a
        // declared import, not a resolved edge, whatever it reads as.
        Edge::Imports { .. } => DECLARED,
        Edge::Ambiguous { .. } => AMBIGUOUS,
        Edge::CtorDi { resolution, .. } => match resolution.as_str() {
            "plain" | "closed" | "open-generic" => EXACT,
            "ambiguous" => AMBIGUOUS,
            // `unresolved`, `infra`, and any word a later release adds: a
            // reader must never be told an injected type bound when it did not.
            _ => UNRESOLVED,
        },
        Edge::Import { .. }
        | Edge::Call { .. }
        | Edge::JsxUse { .. }
        | Edge::Dispatch { .. }
        | Edge::Implements { .. }
        | Edge::Overrides { .. } => EXACT,
        // A bus hop is not read off either site: the publish site and the
        // consumer are recorded facts, and the hop between them is derived.
        Edge::BusHop { .. } => DERIVED,
    }
}
