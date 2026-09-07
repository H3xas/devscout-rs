use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// TS/TSX reference facts (imports, calls, JSX uses, dispatches) and their
// helpers.
//
// Same tree the purpose composition above already parsed, one extra walk, no
// second parse. This section records only what a file SAYS: which specifiers
// it imports and under which local names, which top-level names it exports,
// and which local names it calls / renders as a JSX tag / hands to a
// dispatching call. Nothing here resolves a specifier to a file or a name to
// a declaration -- tsgraph.rs does that across files, the same split
// `extract`/`resolve_graph` already uses for C#.
//
// A TS fragment is tagged `ts: 1` (see `graph::TsFragment`) so the resolver
// split in `resolve_graph` can route it to the TS resolver instead of the C#
// one: the two fragment shapes share no field beyond `defs`, and feeding one
// to the other's resolver would resolve names across languages that have no
// relationship at all.
// ---------------------------------------------------------------------------

/// One exported top-level declaration, in the shape the fragment serializes:
/// `name`, `kind`, `line`, in that order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TsFragmentDef {
    /// The name value.
    pub name: String,
    /// The kind value.
    pub kind: String,
    /// The line value.
    pub line: usize,
    #[serde(rename = "endLine")]
    /// The end line value.
    pub end_line: usize,
}

/// One local name an import statement binds, and the export it names in the
/// source module. `imported` is `"default"` for a default clause and `"*"`
/// for a namespace clause (or a whole-module `require`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TsBinding {
    /// The local value.
    pub local: String,
    /// The imported value.
    pub imported: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Represents `TsImport`.
pub struct TsImport {
    /// The spec value.
    pub spec: String,
    /// The line value.
    pub line: usize,
    /// The bindings value.
    pub bindings: Vec<TsBinding>,
}

/// `export { A as B } from 'm'` re-exports A under the name B: `exported` is
/// what a consumer of THIS file imports, `imported` is what the source module
/// declares.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TsReexportName {
    /// The exported value.
    pub exported: String,
    /// The imported value.
    pub imported: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Represents `TsReexport`.
pub struct TsReexport {
    /// The spec value.
    pub spec: String,
    /// The line value.
    pub line: usize,
    /// The star value.
    pub star: bool,
    /// The names value.
    pub names: Vec<TsReexportName>,
}

/// One recorded reference. Field order (`kind`, `name`, optional `member`,
/// `line`) is significant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TsRef {
    /// The kind value.
    pub kind: String,
    /// The name value.
    pub name: String,
    /// Present only on a qualified reference (`ns.member(...)`,
    /// `<Ns.Thing />`), and serialized between `name` and `line` exactly
    /// where the JS object literal writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// The line value.
    pub line: usize,
}

/// The whole per-file fragment, in serialization order: `defs`, `imports`,
/// `reexports`, `refs`, then `default` when the file has one. The `ts: 1` tag
/// itself lives on the serde type (`graph::TsFragment`), which this converts
/// into -- exactly as `extract::Extraction` converts into `graph::Fragment`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TsFragment {
    /// The routing tag, always `1` and always FIRST -- `resolve_graph`'s
    /// door-level split reads it to send this fragment to the TS resolver
    /// instead of the C# one, and `graph::AnyFragment` reads it to tell the
    /// two cached shapes apart. Required on the read side too: a C#/markup
    /// fragment carries no `ts` key at all, which is what makes the untagged
    /// discrimination total.
    pub ts: u8,
    /// The defs value.
    pub defs: Vec<TsFragmentDef>,
    /// The imports value.
    pub imports: Vec<TsImport>,
    /// The reexports value.
    pub reexports: Vec<TsReexport>,
    /// The refs value.
    pub refs: Vec<TsRef>,
    /// Appended LAST and only when the file has one -- the house rule for
    /// every added fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

impl Default for TsFragment {
    fn default() -> Self {
        TsFragment {
            ts: 1,
            defs: Vec::new(),
            imports: Vec::new(),
            reexports: Vec::new(),
            refs: Vec::new(),
            default: None,
        }
    }
}
