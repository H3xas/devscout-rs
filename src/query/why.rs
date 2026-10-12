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
// The four TS/TSX edge kinds (`call`/`jsx-use`/`dispatch`/`import`) each name
// their own word, appended after `bus-hop` with `test-path` (the `tests` vouch
// a TS file earns by its name) so no earlier word moves.

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
    /// An `impact` row reached only through an imported cross-repo edge --
    /// never folded against the five kinds above, since a foreign file is
    /// never also reached by one of this graph's own edges. The weakest
    /// evidence this vocabulary names, below `uses-member-guess`: a foreign,
    /// producer-asserted fact never outranks anything this crate resolved
    /// for itself.
    ImportedEdge,
    /// A `bus-hop` edge: a publish site's resolved message reaching a
    /// registered consumer/handler def. Appended after `imported-edge`, so
    /// no existing word moves.
    BusHop,
    /// A TS/TSX `call` edge: a call or `new` naming the definition.
    Call,
    /// A TS/TSX `jsx-use` edge: a JSX tag naming the component.
    JsxUse,
    /// A TS/TSX `dispatch` edge: an action creator handed to a dispatching
    /// call.
    Dispatch,
    /// A TS/TSX `import` edge whose bindings name the definition.
    Import,
    /// A `tests` row earned because the file is named as a TypeScript test
    /// file, with neither an attributed def nor a test project behind it.
    TestPath,
}

impl Why {
    /// Every value, in the order the doc comment above lists them.
    pub const ALL: [Why; 19] = [
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
        Why::ImportedEdge,
        Why::BusHop,
        Why::Call,
        Why::JsxUse,
        Why::Dispatch,
        Why::Import,
        Why::TestPath,
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
            Why::ImportedEdge => "imported-edge",
            Why::BusHop => "bus-hop",
            Why::Call => "call",
            Why::JsxUse => "jsx-use",
            Why::Dispatch => "dispatch",
            Why::Import => "import",
            Why::TestPath => "test-path",
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
/// and -- for `uses-member` -- its own `heuristic`/`tier`. Never called with a
/// `ctor-di` edge, the one kind no `refs`/`read`/`impact` row is built from.
pub(crate) fn why_for_edge(edge: &graph::Edge) -> Why {
    match edge {
        graph::Edge::Inherits { .. } => Why::Inherits,
        graph::Edge::UsesType { .. } => Why::UsesType,
        graph::Edge::UsesMember { heuristic, tier, .. } => why_for_uses_member(*heuristic, *tier),
        graph::Edge::Imports { .. } => Why::Imports,
        graph::Edge::Implements { .. } => Why::Implements,
        graph::Edge::Overrides { .. } => Why::Overrides,
        graph::Edge::BusHop { .. } => Why::BusHop,
        graph::Edge::Call { .. } => Why::Call,
        graph::Edge::JsxUse { .. } => Why::JsxUse,
        graph::Edge::Dispatch { .. } => Why::Dispatch,
        graph::Edge::Import { .. } => Why::Import,
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
        graph::Edge::CtorDi { .. } => unreachable!(
            "why_for_edge is never called with a ctor-di edge, the one kind no refs/read/impact row is built from"
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
            "imported-edge",
            "bus-hop",
            "call",
            "jsx-use",
            "dispatch",
            "import",
            "test-path",
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

    #[test]
    fn why_for_edge_reads_bus_hop_as_its_own_word() {
        let hop = graph::Edge::BusHop {
            from_file: "Publisher.cs".to_string(),
            from_line: 1,
            message: "App.Msg".to_string(),
            to: "App.Handler".to_string(),
            to_file: "Handler.cs".to_string(),
            evidence: "base-arg".to_string(),
        };
        assert_eq!(why_for_edge(&hop), Why::BusHop);
        assert_eq!(Why::BusHop.as_str(), "bus-hop");
    }
}
