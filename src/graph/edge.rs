use serde::{Deserialize, Serialize};

use super::fragment_types::is_false;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Represents `Candidate`.
pub struct Candidate {
    /// The id value.
    pub id: String,
    /// The file value.
    pub file: String,
}

/// Which heuristic tier emitted a guess.
///
/// The two differ by an order of magnitude in precision and `heuristic: true`
/// alone cannot tell them apart: `Ext` is C#'s own extension-method lookup run over a recorded
/// `(member, this-type)` bucket -- a real rule, only unverifiable against an
/// out-of-graph receiver -- while `Guess` is the scored tier picking by NAME
/// among the defs that happen to declare a member so called.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HeuristicTier {
    /// Tier (f): extension-method lookup.
    Ext,
    /// The scored tier: a name guess.
    Guess,
}

/// The guess tag, appended LAST on every edge kind that can
/// carry it. `heuristic: true` is set only on an edge a heuristic tier
/// emitted and never writes `heuristic: false`, so this side pairs
/// `skip_serializing_if` with `default`: absent == precise, and a precise
/// edge's bytes are exactly what they were before the tag existed. Only
/// the resolver's heuristic tiers set it today (both
/// `uses-member`), but the flag lives on all three targeted kinds because
/// the query layer's heuristic adjacency is kind-keyed, so a future tier
/// tagging a `uses-type` edge needs no schema change.
///
/// `uses-member` carries two more appended keys, `tier` then `member`, in
/// that order and NOT mirrored onto the other two kinds: neither has a
/// member to name, and no tier tags one today. `tier` splits the umbrella
/// flag into the two tiers a reader can act on differently; `member` names
/// the member the reference reads or calls and is written on every
/// uses-member edge, precise ones included, because "which member" is a fact
/// about the reference rather than about the guess. Both are
/// omit-when-`None`, so a reader of the old shape sees only added keys.
///
/// Fills the slot this module's `Edge::UsesMember::source` reserved: which of
/// two disjoint populations a compiler-vouched edge belongs to. `Semantic`
/// stands in place of whatever the ladder would have produced for the SAME
/// reference the extractor already emitted (a per-reference override, one for
/// one). `SemanticDiscovered` has no extractor reference at all -- the site
/// exists only because the admitted artifact named it -- so it is never
/// counted against the extractor-emitted population in `edges_by_kind` or the
/// syntax-lane audit denominator; only the audit's own `tiers.semantic` block
/// reports it, and separately from `Semantic`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticProvenance {
    /// A same-context compiler fact confirmed or overrode the extractor's own
    /// reference at that occurrence.
    Semantic,
    /// A compiler-verified occurrence the syntax extractor emitted no
    /// reference for.
    SemanticDiscovered,
}

/// Fills the slot this variant previously reserved: `source:
/// Option<SemanticProvenance>`, appended after `member` and omitted when
/// empty, on the same rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Edge {
    #[serde(rename = "inherits")]
    /// Represents `Inherits`.
    Inherits {
        /// The value value.
        from_file: String,
        /// The value value.
        from_line: usize,
        /// The value value.
        to: String,
        /// The value value.
        to_file: String,
        #[serde(default, skip_serializing_if = "is_false")]
        /// The value value.
        heuristic: bool,
    },
    #[serde(rename = "uses-type")]
    /// Represents `UsesType`.
    UsesType {
        /// The value value.
        from_file: String,
        /// The value value.
        from_line: usize,
        /// The value value.
        to: String,
        /// The value value.
        to_file: String,
        #[serde(default, skip_serializing_if = "is_false")]
        /// The value value.
        heuristic: bool,
    },
    #[serde(rename = "uses-member")]
    /// Represents `UsesMember`.
    UsesMember {
        /// The value value.
        from_file: String,
        /// The value value.
        from_line: usize,
        /// The value value.
        to: String,
        /// The value value.
        to_file: String,
        #[serde(default, skip_serializing_if = "is_false")]
        /// The value value.
        heuristic: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Which guess tier emitted this edge -- `Some` exactly when
        /// `heuristic` is true. Build the variant through
        /// `Edge::uses_member`, which derives one from the other.
        tier: Option<HeuristicTier>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// The member this reference reads or calls, on precise and
        /// heuristic edges alike. `None` only for a reference the extractor
        /// recorded no member name for.
        member: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// `Some` exactly on an edge a same-context compiler fact produced
        /// (`Edge::uses_member_semantic`); `None`, and hence entirely absent
        /// from the serialized bytes, on every edge the ladder or a
        /// heuristic tier produced -- a graph built without the semantic
        /// layer is byte-identical to one built before this field existed.
        source: Option<SemanticProvenance>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// The admitted occurrence's own overload signature (the producer's
        /// `target.overloadSignature`, e.g. `"(bool)->void"`), appended LAST
        /// -- present only alongside `source`, and only when the admitted
        /// occurrence named one. Two same-line overloads of one member on
        /// the SAME type resolve to the SAME `to`/`to_file` (the type-level
        /// identity a `uses-member` edge already carries), so without this
        /// field their projected edges would be byte-identical and a reader
        /// could never tell them apart as the two distinct facts they are.
        /// `None` on every syntax or heuristic edge, which keeps a graph
        /// built without the semantic layer byte-identical to one built
        /// before this field existed.
        overload_signature: Option<String>,
    },
    #[serde(rename = "imports")]
    /// The value value.
    Imports {
        /// The source file.
        from_file: String,
        /// The source line.
        from_line: usize,
        /// The imported target.
        target: String,
    },
    #[serde(rename = "ambiguous")]
    /// Represents `Ambiguous`.
    Ambiguous {
        /// The value value.
        origin: String,
        /// The value value.
        from_file: String,
        /// The value value.
        from_line: usize,
        /// The value value.
        raw: String,
        /// The value value.
        candidates: Vec<Candidate>,
        /// The value value.
        candidate_count: usize,
    },
    /// Constructor-parameter DI resolution (see resolve.rs's
    /// `resolve_ctor_param`). Field order (`kind`, `from_file`, `from_line`,
    /// `iface`, `resolution`, `args`, `to`, `candidates`) is significant --
    /// it fixes the serialized field order.
    /// `iface` is the injected type's bare identifier; `args` its closed
    /// generic arguments, present only when it has any; `to` the resolved
    /// implementation's def id, present only for 'plain'/'closed'/
    /// 'open-generic'; `candidates` the capped, sorted tie list, present only
    /// for 'ambiguous'.
    #[serde(rename = "ctor-di")]
    CtorDi {
        /// The value value.
        from_file: String,
        /// The value value.
        from_line: usize,
        /// The value value.
        iface: String,
        /// The value value.
        resolution: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// The value value.
        args: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// The value value.
        to: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        /// The value value.
        candidates: Vec<Candidate>,
    },
    /// A TS/TSX module import. Distinct from `imports` (C#'s
    /// `using` directive, which names a NAMESPACE and resolves to no file):
    /// `target` is the specifier as written and `to_file` the file it
    /// resolved to. `via` is appended LAST and present only on a
    /// barrel-followed edge, naming the module the source literally imported
    /// so a reader can tell the routing table apart from the dependency.
    #[serde(rename = "import")]
    Import {
        /// The value value.
        from_file: String,
        /// The value value.
        from_line: usize,
        /// The value value.
        target: String,
        /// The value value.
        to_file: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// The value value.
        via: Option<String>,
    },
    /// A call or `new` naming an imported or locally-exported
    /// declaration.
    #[serde(rename = "call")]
    /// The value value.
    Call {
        /// The source file.
        from_file: String,
        /// The source line.
        from_line: usize,
        /// The target definition identifier.
        to: String,
        /// The target file.
        to_file: String,
    },
    /// A JSX tag naming a component declaration.
    #[serde(rename = "jsx-use")]
    /// The value value.
    JsxUse {
        /// The source file.
        from_file: String,
        /// The source line.
        from_line: usize,
        /// The target definition identifier.
        to: String,
        /// The target file.
        to_file: String,
    },
    /// An action creator or thunk handed to a dispatching call
    /// (`dispatch(...)`, `ofType(...)`).
    #[serde(rename = "dispatch")]
    /// The value value.
    Dispatch {
        /// The source file.
        from_file: String,
        /// The source line.
        from_line: usize,
        /// The target definition identifier.
        to: String,
        /// The target file.
        to_file: String,
    },
    /// The implementation of a service interface: either a type-level fact
    /// (`member` absent) read off a two-type-argument DI registration call
    /// (`services.AddScoped<IContract, Impl>()`), recorded against the
    /// IMPLEMENTATION type's own declaring file and line -- the same
    /// convention `inherits` uses for its own base-list reference, applied
    /// here to a fact the resolver derived rather than one it read directly
    /// off that type's own base list -- or a member-level fact (`member`
    /// present, naming the satisfied interface member) connecting an
    /// implementing member to the interface member it satisfies. Never a
    /// guess: an ambiguous name-and-arity match emits nothing rather than
    /// picking a candidate. `to`/`to_file` name the interface.
    #[serde(rename = "implements")]
    Implements {
        /// The value value.
        from_file: String,
        /// The value value.
        from_line: usize,
        /// The value value.
        to: String,
        /// The value value.
        to_file: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// The interface member this implementation satisfies; absent for
        /// the type-level, registration-driven fact.
        member: Option<String>,
    },
    /// An `override` member connected to the nearest in-graph base member of
    /// the same name and arity, matched the same way `implements` is: two or
    /// more candidates at that arity emit nothing. `to`/`to_file` name the
    /// base type declaring the overridden member; `member` names it.
    #[serde(rename = "overrides")]
    Overrides {
        /// The value value.
        from_file: String,
        /// The value value.
        from_line: usize,
        /// The value value.
        to: String,
        /// The value value.
        to_file: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// The value value.
        member: Option<String>,
    },
}

impl Edge {
    /// The guess tier that emitted this edge, or `None` on a precise one --
    /// and on every kind that carries no tier at all, which is every kind but
    /// `uses-member`.
    pub fn tier(&self) -> Option<HeuristicTier> {
        match self {
            Edge::UsesMember { tier, .. } => *tier,
            _ => None,
        }
    }

    /// Whether the edge declares itself a guess, across the three kinds that
    /// can carry the flag.
    pub fn is_heuristic(&self) -> bool {
        match self {
            Edge::Inherits { heuristic, .. }
            | Edge::UsesType { heuristic, .. }
            | Edge::UsesMember { heuristic, .. } => *heuristic,
            _ => false,
        }
    }

    /// The one way to build a `uses-member` edge. `heuristic` is DERIVED from
    /// `tier` rather than passed alongside it, so the two cannot disagree: a
    /// tagged edge is always flagged, a flagged edge always names its tier,
    /// and no emit site can grow a third state by forgetting a field.
    pub fn uses_member(
        from_file: String,
        from_line: usize,
        to: String,
        to_file: String,
        member: Option<String>,
        tier: Option<HeuristicTier>,
    ) -> Edge {
        Edge::UsesMember {
            from_file,
            from_line,
            to,
            to_file,
            heuristic: tier.is_some(),
            tier,
            member,
            source: None,
            overload_signature: None,
        }
    }

    /// The one way to build a compiler-vouched `uses-member` edge: never
    /// heuristic, never tiered (a confirmed compiler fact is not a guess),
    /// always carrying `source` so a reader can tell it apart from every
    /// edge the ladder or a heuristic tier produced. `overload_signature`
    /// carries the admitted occurrence's own signature, when it named one --
    /// the exact identity that keeps two same-line overloads of one member
    /// distinguishable once projected.
    pub fn uses_member_semantic(
        from_file: String,
        from_line: usize,
        to: String,
        to_file: String,
        member: Option<String>,
        provenance: SemanticProvenance,
        overload_signature: Option<String>,
    ) -> Edge {
        Edge::UsesMember {
            from_file,
            from_line,
            to,
            to_file,
            heuristic: false,
            tier: None,
            member,
            source: Some(provenance),
            overload_signature,
        }
    }

    /// Which population produced this edge, when it names one at all --
    /// `None` for every kind that is not a compiler-vouched `uses-member`
    /// edge, including an ordinary precise or heuristic one.
    pub fn provenance(&self) -> Option<SemanticProvenance> {
        match self {
            Edge::UsesMember { source, .. } => *source,
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// Represents `EdgesByKind`.
pub struct EdgesByKind {
    /// The inherits value.
    pub inherits: usize,
    #[serde(rename = "uses-type")]
    /// The uses type value.
    pub uses_type: usize,
    /// The imports value.
    pub imports: usize,
    #[serde(rename = "uses-member")]
    /// The uses member value.
    pub uses_member: usize,
    /// Appended LAST, matching the `ctor-di` slot in the fixed key order.
    #[serde(rename = "ctor-di")]
    pub ctor_di: usize,
    /// The type-level and member-level `implements` edges, counted
    /// together -- appended after `ctor-di`, before the optional TS keys.
    #[serde(default)]
    pub implements: usize,
    /// The member-level `overrides` edges, appended right after
    /// `implements`, same rule.
    #[serde(default)]
    pub overrides: usize,
    /// The four TS edge counts, appended after `overrides` in this exact order
    /// and ONLY when the repo carries a TS fragment at all. A C#-only repo's
    /// stats block omits them entirely -- which is why these are `Option` and
    /// not a plain `0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The call value.
    pub call: Option<usize>,
    #[serde(rename = "jsx-use", default, skip_serializing_if = "Option::is_none")]
    /// The jsx use value.
    pub jsx_use: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The dispatch value.
    pub dispatch: Option<usize>,
}

/// `heuristic_edge_count` split by the tier that emitted each edge, in the
/// fixed key order every tier-keyed output uses (ext, then guess).
///
/// Both keys are always written, and `ext + guess` equals `heuristic_edge_count` --
/// including after the heuristic-side dedup, which decrements the dropped
/// edge's own tier.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct HeuristicByTier {
    /// Edges tier (f) emitted.
    pub ext: usize,
    /// Edges the scored tier emitted.
    pub guess: usize,
}

/// The resolve-time consumption path's own run-level counters: appended to
/// `Stats` only when the semantic layer actually loaded for this run, so a
/// syntax-only build (no admitted artifact, or a stale one) carries no
/// `semantic` key at all -- the same omit-when-absent rule every
/// enrichment-layer fact follows, and what keeps a graph built without the layer
/// byte-identical to one built before it existed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticStats {
    /// Per-reference overrides the same-context authority rule confirmed.
    pub confirmed: usize,
    /// How many of those confirmations displaced a different target the
    /// ladder itself had already bound -- the disagreement diagnostic count.
    pub disagreements: usize,
    /// Compiler-discovered edges: admitted, confirmed occurrences the
    /// extractor emitted no reference for at all. Never pooled with
    /// `confirmed` in any count this struct reports.
    pub discovered: usize,
}
