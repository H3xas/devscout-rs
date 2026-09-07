// The closed `why` vocabulary every hit row on `refs`/`read`/`impact`/`tests`
// carries in `--json`: which rule or tier produced the row, spelled once
// here -- the same discipline `outcome.rs` applies to the `outcome` field, so
// no caller can typo a word this vocabulary does not name.
//
// Every value is derived from what the graph already persists on the edge
// that produced the row: its kind, and for a `uses-member` edge its tier.
// Nothing here reads a new fact the resolver did not already record, and
// nothing is stamped on an ambiguous or candidate-count row -- those already
// carry their own `origin`/`candidateCount` fields describing why they are
// UNRESOLVED, a different question from which rule produced a settled hit.
//
// A TS/TSX-only edge kind (`import`/`call`/`jsx-use`/`dispatch`) names no
// word here: `query::index` never admits those kinds into the
// inbound/outbound adjacency any of the four verbs read (see its own
// `Edge::Import | Edge::Call | Edge::JsxUse | Edge::Dispatch => {}` arm), so a
// row built from one never exists for `why` to describe.

use crate::graph;

/// One of the words a hit row's `why` field names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// A base-list (`inherits`) edge.
    Inherits,
    /// A `uses-type` edge.
    UsesType,
    /// A precise (non-heuristic) `uses-member` edge.
    UsesMemberPrecise,
    /// A `uses-member` edge the extension-method tier guessed.
    UsesMemberExt,
    /// A `uses-member` edge the scored-guess tier guessed (or one that
    /// carries no tier at all -- see `row_tier`'s own rule for that case).
    UsesMemberGuess,
    /// A constructor-injection (`ctor-di`) edge.
    CtorDi,
    /// A C# `using` directive (an `imports` edge).
    Imports,
    /// The def's own declaration span -- `read` only, never an edge at all.
    Declaration,
    /// A `tests` row earned because a def declared in the file carries a
    /// test-runner attribute.
    TestAttribute,
    /// A `tests` row earned because the project model places the file's
    /// unit in a project marked `test`, with no attributed def of its own.
    TestProject,
    /// An `implements` edge: a service registration's implementation type, or
    /// a member satisfying an interface member. Appended after the words that
    /// pre-date the edge kind, so no existing word moves.
    Implements,
    /// An `overrides` edge: a member overriding the nearest in-graph base
    /// member of the same name and arity.
    Overrides,
}

impl Why {
    /// Every value, in the order the doc comment above lists them.
    pub const ALL: [Why; 12] = [
        Why::Inherits,
        Why::UsesType,
        Why::UsesMemberPrecise,
        Why::UsesMemberExt,
        Why::UsesMemberGuess,
        Why::CtorDi,
        Why::Imports,
        Why::Declaration,
        Why::TestAttribute,
        Why::TestProject,
        Why::Implements,
        Why::Overrides,
    ];

    /// The exact `--json` word for this value.
    pub fn as_str(self) -> &'static str {
        match self {
            Why::Inherits => "inherits",
            Why::UsesType => "uses-type",
            Why::UsesMemberPrecise => "uses-member-precise",
            Why::UsesMemberExt => "uses-member-ext",
            Why::UsesMemberGuess => "uses-member-guess",
            Why::CtorDi => "ctor-di",
            Why::Imports => "imports",
            Why::Declaration => "declaration",
            Why::TestAttribute => "test-attribute",
            Why::TestProject => "test-project",
            Why::Implements => "implements",
            Why::Overrides => "overrides",
        }
    }
}

/// The `why` a `uses-member` row or edge reports, from its own tier: `None`
/// (precise), `Some(Ext)`, or `Some(Guess)` -- the exact three states
/// `row_tier` already folds a row's edges down to, so a caller holding either
/// a row's `tier` field or an edge's own `.tier()` reads the same word from
/// it. `heuristic` decides the precise/guessed split; an edge or row that
/// says it was guessed but names no tier (a graph written before schema 2,
/// or a hand-built row exercising that case) still reports the WEAKER word,
/// never the precise one -- the same direction `row_tier` itself never
/// rounds.
pub(crate) fn why_for_uses_member(heuristic: bool, tier: Option<graph::HeuristicTier>) -> Why {
    if !heuristic {
        return Why::UsesMemberPrecise;
    }
    match tier {
        Some(graph::HeuristicTier::Ext) => Why::UsesMemberExt,
        _ => Why::UsesMemberGuess,
    }
}

/// The `why` value for the edge that produced a row, derived from its kind
/// and -- for `uses-member` -- its own `heuristic`/`tier`. Only ever called
/// with an `inherits`/`uses-type`/`uses-member`/`imports`/`ambiguous` edge,
/// the only kinds that ever populate a `refs`/`read`/`impact` row; every
/// other kind is unreachable here by construction (see the module header).
pub(crate) fn why_for_edge(edge: &graph::Edge) -> Why {
    match edge {
        graph::Edge::Inherits { .. } => Why::Inherits,
        graph::Edge::UsesType { .. } => Why::UsesType,
        graph::Edge::UsesMember { heuristic, tier, .. } => why_for_uses_member(*heuristic, *tier),
        graph::Edge::Imports { .. } => Why::Imports,
        graph::Edge::Implements { .. } => Why::Implements,
        graph::Edge::Overrides { .. } => Why::Overrides,
        // An ambiguous edge's `origin` is the same ref-kind string the ladder
        // steps record for a resolved edge ("inherits"/"uses-type"/
        // "uses-member"); an ambiguous edge is never heuristic (ambiguity and
        // the guess tiers are mutually exclusive resolution outcomes), so the
        // `uses-member` fallback here is always the precise word.
        graph::Edge::Ambiguous { origin, .. } => match origin.as_str() {
            "inherits" => Why::Inherits,
            "uses-type" => Why::UsesType,
            _ => Why::UsesMemberPrecise,
        },
        _ => unreachable!(
            "why_for_edge is only called with inherits/uses-type/uses-member/imports/implements/overrides/ambiguous edges, the only kinds that ever populate a refs/read/impact row"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_why_word_is_one_of_the_closed_vocabulary() {
        const EXPECTED: &[&str] = &[
            "inherits",
            "uses-type",
            "uses-member-precise",
            "uses-member-ext",
            "uses-member-guess",
            "ctor-di",
            "imports",
            "declaration",
            "test-attribute",
            "test-project",
            "implements",
            "overrides",
        ];
        let words: Vec<&str> = Why::ALL.iter().map(|w| w.as_str()).collect();
        for word in &words {
            assert!(
                EXPECTED.contains(word),
                "{word} is not in the closed why vocabulary"
            );
        }
        assert_eq!(
            words.len(),
            EXPECTED.len(),
            "ALL must name every vocabulary word exactly once"
        );
    }

    #[test]
    fn why_for_edge_reads_uses_member_tier_the_same_way_row_tier_folds_it() {
        let precise = graph::Edge::uses_member(
            "a.cs".to_string(),
            1,
            "T".to_string(),
            "b.cs".to_string(),
            None,
            None,
        );
        assert_eq!(why_for_edge(&precise), Why::UsesMemberPrecise);

        let ext = graph::Edge::uses_member(
            "a.cs".to_string(),
            1,
            "T".to_string(),
            "b.cs".to_string(),
            None,
            Some(graph::HeuristicTier::Ext),
        );
        assert_eq!(why_for_edge(&ext), Why::UsesMemberExt);

        let guess = graph::Edge::uses_member(
            "a.cs".to_string(),
            1,
            "T".to_string(),
            "b.cs".to_string(),
            None,
            Some(graph::HeuristicTier::Guess),
        );
        assert_eq!(why_for_edge(&guess), Why::UsesMemberGuess);
    }

    #[test]
    fn why_for_edge_reads_ambiguous_origin_as_its_ref_kind() {
        let inherits = graph::Edge::Ambiguous {
            origin: "inherits".to_string(),
            from_file: "a.cs".to_string(),
            from_line: 1,
            raw: "Base".to_string(),
            candidates: Vec::new(),
            candidate_count: 2,
        };
        assert_eq!(why_for_edge(&inherits), Why::Inherits);

        let uses_type = graph::Edge::Ambiguous {
            origin: "uses-type".to_string(),
            from_file: "a.cs".to_string(),
            from_line: 1,
            raw: "T".to_string(),
            candidates: Vec::new(),
            candidate_count: 2,
        };
        assert_eq!(why_for_edge(&uses_type), Why::UsesType);

        let uses_member = graph::Edge::Ambiguous {
            origin: "uses-member".to_string(),
            from_file: "a.cs".to_string(),
            from_line: 1,
            raw: "M".to_string(),
            candidates: Vec::new(),
            candidate_count: 2,
        };
        assert_eq!(why_for_edge(&uses_member), Why::UsesMemberPrecise);
    }
}
