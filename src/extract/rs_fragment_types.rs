use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// The Rust fragment data model: one record per item with its span, and
// nothing else. A Rust fragment carries definitions only; no reference,
// import or module fact is recorded, so no resolver reads it.
//
// A Rust fragment is tagged `rs: 1` so `graph::AnyFragment` can tell it apart
// from the other two cached shapes: a TypeScript fragment requires `ts`, a
// C#/markup fragment requires `usings`, and this one requires `rs` and
// carries neither of the others' required keys.
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

/// The whole per-file Rust fragment, in serialization order: `rs`, `items`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsFragment {
    /// The shape tag, always `1` and always first. Required on the read side,
    /// which is what keeps the untagged fragment union total.
    pub rs: u8,
    /// Every recorded item, in source order.
    pub items: Vec<RsItem>,
}

impl Default for RsFragment {
    fn default() -> Self {
        RsFragment {
            rs: 1,
            items: Vec::new(),
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
