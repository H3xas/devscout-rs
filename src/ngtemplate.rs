// The Angular template scan: which component members a template reads, and
// which sites of it the index cannot bind. It is pure over the template text,
// so an external `.html` template and an inline `template:` string go through
// the same function; the caller supplies the line the text starts on.
//
// Locals are resolved here, not at graph time, because what a local hides
// depends on the template text alone. A scan therefore holds only root
// identifier reads that no local hides, and the gaps the scanner itself found.
// Whether a read names a member is the graph join's question
// (`tsgraph::templates`), since only the component knows its members.

use serde::{Deserialize, Serialize};

mod expr;
mod markup;
mod micro;
mod scope;

#[cfg(test)]
mod tests;

/// Why a template site produced no edge. Declared in the order graph.json
/// sorts rows of one line by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GapReason {
    /// A line that opens a built-in control flow block (`@if`, `@for`, ...).
    ControlFlowBlock,
    /// A binding or interpolation expression the scanner cannot parse.
    UnparsedExpression,
    /// A literal `templateUrl` that names no file in the indexed root.
    TemplateUrlUnresolved,
    /// A `templateUrl` whose value is not one plain string literal.
    TemplateUrlNonLiteral,
    /// A read of a name that is neither a local nor a component member.
    UnknownMember,
    /// A `template` whose value is not one plain string literal.
    InlineTemplateSubstitution,
}

/// One root identifier a template reads, with the 1-based line it is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateRead {
    /// The identifier as written.
    pub name: String,
    /// The line of the identifier itself, also inside a multi-line value.
    pub line: usize,
}

/// One site the scan could not bind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateGap {
    /// The 1-based line of the site.
    pub line: usize,
    /// Why it gave no edge.
    pub reason: GapReason,
}

/// What one template reads and where it could not be read. Both lists are in
/// source order and omitted when empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateScan {
    /// The reads no template local hides.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<TemplateRead>,
    /// The scanner's own gaps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<TemplateGap>,
}

/// Scans one template whose first line is `first_line` in its file.
pub fn scan_template(text: &str, first_line: usize) -> TemplateScan {
    scope::walk(markup::events(text, first_line))
}
