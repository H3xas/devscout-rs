use serde::{Deserialize, Serialize};

use super::rs_facts_types::{RsBlock, RsImpl, RsSite, RsUse};

// ---------------------------------------------------------------------------
// The Rust fragment data model: one record per item with its span, then the
// resolution facts (`rs_facts_types`) the edge pass reads, each omitted when
// empty.
//
// A Rust fragment is tagged `rs: 1` so `graph::AnyFragment` can tell it apart
// from the other two cached shapes: a TypeScript fragment requires `ts`, a
// C#/markup fragment requires `usings`, and this one requires `rs` and
// carries neither of the others' required keys. Reference sites are keyed
// `sites` for the same reason: `refs` is a key the other two arms require.
// ---------------------------------------------------------------------------

/// One recorded Rust item, in serialization order: `name`, `kind`, `path`,
/// `line`, `nameLine`, `endLine`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsItem {
    /// The item's own name, without a raw-identifier prefix.
    pub name: String,
    /// The Rust keyword for the item (`fn`, `struct`, `macro_rules`, ...).
    pub kind: String,
    /// The `::`-joined path inside the file: enclosing inline modules, then
    /// the owner segment, then the name, with a `-N` suffix when an earlier
    /// item in the same file already took the path.
    pub path: String,
    /// First line of the span: the first outer doc comment or attribute
    /// directly above the item, else the item's own first line.
    pub line: usize,
    /// The line of the name token.
    #[serde(rename = "nameLine")]
    pub name_line: usize,
    /// The item's last line.
    #[serde(rename = "endLine")]
    pub end_line: usize,
}

/// The whole per-file Rust fragment, in serialization order: `rs`, `items`,
/// then the resolution facts, each omitted when empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsFragment {
    /// The shape tag, always `1` and always first. Required on the read side,
    /// which is what keeps the untagged fragment union total.
    pub rs: u8,
    /// Every recorded item, in source order.
    pub items: Vec<RsItem>,
    /// The item path of each `mod x;` declaration (a module with no body).
    #[serde(rename = "modFiles", default, skip_serializing_if = "Vec::is_empty")]
    pub mod_files: Vec<String>,
    /// The item path of each tuple or unit struct: the structs that also
    /// live in the value namespace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ctors: Vec<String>,
    /// The item path of each function carrying a `#[test]` attribute.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<String>,
    /// Every `impl` block whose self type has a path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub impls: Vec<RsImpl>,
    /// Every flattened `use` leaf, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uses: Vec<RsUse>,
    /// Every block that directly holds a `use` declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<RsBlock>,
    /// Every reference site, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sites: Vec<RsSite>,
    /// Inline module scopes with an item macro whose names are unenumerable.
    #[serde(
        rename = "opaqueScopes",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub opaque_scopes: Vec<String>,
}

impl Default for RsFragment {
    fn default() -> Self {
        RsFragment {
            rs: 1,
            items: Vec::new(),
            mod_files: Vec::new(),
            ctors: Vec::new(),
            tests: Vec::new(),
            impls: Vec::new(),
            uses: Vec::new(),
            blocks: Vec::new(),
            sites: Vec::new(),
            opaque_scopes: Vec::new(),
        }
    }
}

/// One Rust file's purpose (`None` when the file has neither a leading
/// comment nor a file-level item) and its fragment, off one parse.
#[derive(Debug, Clone, PartialEq)]
pub struct RsFileExtraction {
    /// The purpose value.
    pub purpose: Option<String>,
    /// The fragment value.
    pub fragment: RsFragment,
}
