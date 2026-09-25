// The data model every other `audit` submodule shares: the reduced
// `graph.json`/oracle-record row shapes scoring reads, and the filesystem
// inputs `load` gathers and `score` consumes. No I/O and no scoring logic
// live here -- see `load.rs` and `score.rs`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A `uses-member` edge's resolver tier. `Heuristic` is the legacy shape: a
/// `heuristic: true` edge with no `tier` string at all, which is how every
/// non-precise edge was written before the `tier` key and the `Ext`/`Guess`
/// split it carries existed. Both shapes must keep scoring, so this enum
/// spans them and `tier_of` (in `load.rs`) is the one place that maps either
/// onto it.
///
/// `Semantic` and `SemanticDiscovered` are the enriched lane's own two tiers,
/// appended last so no existing row or JSON key moves position. They are two
/// tiers, not one, so a per-reference override of an already-emitted
/// reference (`Semantic`) and a compiler-discovered site the extractor never
/// emitted a reference for at all (`SemanticDiscovered`) structurally cannot
/// share one `TierStats` row -- the two populations must never be pooled,
/// and keeping them in separate enum variants makes that true by
/// construction rather than by convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    Precise,
    Ext,
    Guess,
    Heuristic,
    Semantic,
    SemanticDiscovered,
}

impl Tier {
    /// Fixed iteration/display order for every tier-keyed
    /// output (text table rows, `tiers` JSON keys): precise, ext, guess,
    /// heuristic, then the two enriched-lane tiers, appended last.
    pub const ORDER: [Tier; 6] = [
        Tier::Precise,
        Tier::Ext,
        Tier::Guess,
        Tier::Heuristic,
        Tier::Semantic,
        Tier::SemanticDiscovered,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Tier::Precise => "precise",
            Tier::Ext => "ext",
            Tier::Guess => "guess",
            Tier::Heuristic => "heuristic",
            Tier::Semantic => "semantic",
            Tier::SemanticDiscovered => "semantic-discovered",
        }
    }
}

/// One `uses-member` edge out of graph.json, reduced to the fields scoring
/// needs. `to_file`/`to` are the graph's own def id and its declaring file --
/// `graph.rs`'s `Def.file` is the def's first-insertion file for a partial
/// class, so a member declared in a second `also_in` file is joined on that
/// first file here too -- the join is on def id alone, never on the declaring
/// file, precisely so a partial class does not split into two answers.
/// `member` is schema 2's own member-name key (`graph.rs`'s
/// `Edge::UsesMember.member`) -- `None` for a schema-1 edge (no `member` key
/// on disk at all) or the rare reference the extractor recorded no member
/// name for; either way `None` makes this edge's second half of the match
/// rule (`member_matches`, in `scoring.rs`) unconstrained, which is what
/// keeps a schema-1 graph's scoring byte-identical to before this field
/// existed.
pub struct EdgeRow {
    pub from_file: String,
    pub from_line: usize,
    pub to: String,
    pub to_file: String,
    pub tier: Tier,
    pub member: Option<String>,
}

/// A graph.json def, reduced to what scoring needs -- and, doubling as the
/// shape `--defs <defs.jsonl>` deserializes into directly (that oracle
/// record has the same four field names; see `load.rs`). `test` is "does
/// this def carry at least one method with a devscout-recognized test
/// attribute" (graph.json's `testMethods` array, non-empty) for a
/// graph-sourced row, or the oracle's own equivalent Roslyn-derived flag for
/// an oracle-sourced one.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct DefRow {
    pub id: String,
    pub file: String,
    // Carried through for parity with the oracle's on-disk defs.jsonl
    // record; no scoring rule reads it back (grouping by kind uses the
    // oracle ref's own `targetKind`, not this).
    #[allow(dead_code)]
    pub kind: String,
    #[serde(default)]
    pub test: bool,
}

/// One line of `refs.jsonl`, the oracle's ground-truth member-reference
/// records -- reduced (see the module header) to what scoring reads.
/// `target`/`targetKind`/`targetFile` are `Option` because the oracle writes
/// `null` for an unknown value; `member` is not --
/// `tools/scout-semantic`'s `Records.cs` declares it a non-nullable `string`,
/// and every syntax shape the oracle walks (`a.M`, `?.M`, a bare `M(...)`)
/// names a member by construction -- and every other field here is always
/// present on a real oracle record too.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OracleRef {
    pub file: String,
    #[serde(rename = "startLine")]
    pub start_line: usize,
    pub shape: String,
    #[serde(rename = "receiverKind")]
    pub receiver_kind: String,
    pub member: String,
    pub target: Option<String>,
    #[serde(rename = "targetKind")]
    pub target_kind: Option<String>,
    #[serde(rename = "targetFile")]
    pub target_file: Option<String>,
    pub external: bool,
    pub ambiguous: bool,
}

/// One line of `units.jsonl` -- a compiled project, reduced to what the
/// structural check needs: `status` (ok/failed unit counts, and whether a
/// file belongs to a failed unit at all -- excluded from the audit
/// universe), `refs` (other unit names this unit's `ProjectReferences` name,
/// the raw material for the reachability closure `reach` builds), and
/// `files` (the file->unit membership map `file_to_unit` builds), and `name`
/// (the key `refs`/`file_to_unit` join on). `test`/`tfm`/`diagnostics` from
/// the oracle's own schema are not carried: nothing here reads a unit's own
/// `test` flag (only a *def's* `test` flag, via `DefRow`, matters to the
/// fallback structural check).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Unit {
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub refs: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

/// Filesystem inputs to one audit run -- everything `load` gathers and
/// `score` consumes. Held together so `score` stays a single-argument pure
/// function: no field here is touched by any I/O once this value exists.
pub struct Inputs {
    pub root: PathBuf,
    /// Defs from graph.json (`devscout`'s own understanding of the in-tree
    /// type/enum-member universe).
    pub graph_defs: Vec<DefRow>,
    /// Defs from `--defs <defs.jsonl>` (Roslyn's), if given; empty otherwise
    /// -- the structural fallback's `test`/`file_has_test_def` lookups
    /// prefer this source when a def id appears in both (see `score.rs`).
    pub oracle_defs: Vec<DefRow>,
    /// `uses-member` edges from graph.json.
    pub edges: Vec<EdgeRow>,
    /// Every record `--semantic <refs.jsonl>` carried, BEFORE the universe
    /// filter `score` applies -- `score` is the one place that drops a
    /// record and counts the drop, so the raw set is what a caller-supplied
    /// `Inputs` should also carry.
    pub records: Vec<OracleRef>,
    /// Units from `--units <units.jsonl>`, if given; empty otherwise.
    pub units: Vec<Unit>,
    /// File universe an oracle record's `file` must fall inside to be
    /// scored: the manifest's mapped-file set (or, with no manifest, every
    /// file graph.json mentions), minus the files of any unit whose
    /// `status` is not `"ok"`.
    pub universe: HashSet<String>,
    /// `"enriched"` when graph.json's own `stats.semantic` block is present
    /// (an admitted compiler-facts artifact loaded for the run that produced
    /// this graph), else `"syntax"`. Read directly off the graph, never
    /// inferred from whether any edge happens to carry a `Semantic`/
    /// `SemanticDiscovered` tier -- a run that admitted an artifact but found
    /// nothing to confirm is still an enriched-lane run, not a syntax one.
    pub lane: &'static str,
    /// The admitted compiler-facts artifact's own per-occurrence `shape`
    /// (`"access"`, `"conditional"`, `"invocation"` or `"identifier"` --
    /// `tools/scout-semantic/CompilerOccurrences.cs`'s own vocabulary, not
    /// the oracle's), keyed by the SAME `(file, name-line, target member)`
    /// triple `SemanticDiscovered` edges carry -- `(from_file, from_line,
    /// member)` on `EdgeRow`. Read directly from this checkout's admitted
    /// artifact (`graph::read_compiler_facts`), independent of graph.json
    /// and never touching `src/semantic/`'s own parse; empty when no
    /// artifact is admitted, or for a syntax-only run. `score` reads this
    /// only for `Tier::SemanticDiscovered` edges, to separate a discovered
    /// edge the oracle's own walker has no vocabulary for at all
    /// (`"identifier"`) from one it does -- see `score.rs`'s classification
    /// loop.
    pub discovered_shapes: HashMap<(String, usize, String), String>,
    /// Whether `score` should build `AuditReport.fp_sites`. `load` always
    /// sets this `false`; `cmd_audit` flips it on after `load` returns, only
    /// when `--fp-sites` was given, since the flag is the one thing `load`
    /// itself never sees. A caller-built `Inputs` that never sets it wants
    /// no rows, matching the flag's own default.
    pub collect_fp_sites: bool,
}

/// `load`'s filesystem arguments -- the resolved, already-`-C`-aware paths
/// for the three input files `cmd_audit` accepts.
pub struct AuditOptions<'a> {
    pub semantic: &'a Path,
    pub units: Option<&'a Path>,
    pub defs: Option<&'a Path>,
}
