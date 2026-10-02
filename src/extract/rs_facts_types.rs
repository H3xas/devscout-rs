use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// The Rust fragment's resolution facts: what the module tree, the `use`
// scope and the edge pass read, recorded per file so the graph can be built
// from cached fragments alone.
//
// A "scope" is the inline-module path inside the file (`tests`,
// `outer::inner`), empty at the file root. A "block" indexes the fragment's
// `blocks` list: only blocks that directly hold a `use` declaration are
// recorded, because nothing else inside a function body can bind a name to
// a recorded item. Every field is omitted when empty, so a file with none of
// these facts serializes as it did before they existed.
// ---------------------------------------------------------------------------

fn is_zero_len(s: &str) -> bool {
    s.is_empty()
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One `impl` block whose self type has a path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsImpl {
    /// The inline-module path the block sits in.
    #[serde(default, skip_serializing_if = "is_zero_len")]
    pub scope: String,
    /// The self type's path as written, without generic arguments,
    /// references, raw pointers or `dyn`.
    #[serde(rename = "selfType")]
    pub self_type: String,
    /// The item paths recorded inside the block, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<String>,
}

/// One `mod x;` declaration whose file a `path` attribute decides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsModPath {
    /// The declaration's item path, as listed in `modFiles`.
    pub item: String,
    /// The literal of its first `#[path = "..."]`; absent when the file
    /// cannot be told, and then the declaration reaches nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// One flattened `use` leaf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsUse {
    /// The inline-module path the declaration sits in.
    #[serde(default, skip_serializing_if = "is_zero_len")]
    pub scope: String,
    /// The recorded block that directly holds the declaration, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<usize>,
    /// The imported path joined by `::`; a leading `::` is kept.
    pub path: String,
    /// The bound name (the rename, else the last segment), or `*` for a glob.
    pub name: String,
    /// The line of the bound name.
    pub line: usize,
}

/// One block that directly holds a `use` declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsBlock {
    /// The nearest enclosing recorded block, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
}

/// What a reference site's path is used as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RsRole {
    /// The callee of a call expression, or a macro-argument call.
    Call,
    /// Any other path expression.
    Value,
    /// A type position.
    Type,
    /// The name of a struct literal.
    Lit,
    /// The trait path of `impl Trait for T`.
    Impl,
    /// A path inside a pattern: only its qualifier segments count.
    Pat,
}

/// One reference site: a path the edge pass tries to bind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsSite {
    /// What the path is used as.
    pub role: RsRole,
    /// The path as written, generic arguments dropped, joined by `::`.
    pub path: String,
    /// The line of the last segment.
    pub line: usize,
    /// One line per segment, written only when the path spans lines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<usize>,
    /// The inline-module path the site sits in.
    #[serde(default, skip_serializing_if = "is_zero_len")]
    pub scope: String,
    /// The innermost recorded block around the site, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<usize>,
    /// For a `Self`-rooted path inside an `impl`, that impl's index.
    #[serde(rename = "selfImpl", default, skip_serializing_if = "Option::is_none")]
    pub self_impl: Option<usize>,
    /// For a `Self`-rooted path inside a trait, the trait's item path.
    #[serde(rename = "selfTrait", default, skip_serializing_if = "Option::is_none")]
    pub self_trait: Option<String>,
    /// Whether the site was found inside a macro invocation's arguments.
    #[serde(default, skip_serializing_if = "is_false")]
    pub mac: bool,
}

impl RsSite {
    /// The line of each segment: `lines` when written, else `line` for all.
    pub fn segment_lines(&self) -> Vec<usize> {
        let count = self.path.split("::").count();
        if self.lines.len() == count {
            self.lines.clone()
        } else {
            vec![self.line; count]
        }
    }
}
