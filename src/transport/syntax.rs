//! Syntax readers shared by the site walk and the constant reader: node
//! text, string literals and the pieces a joined string value is built from.

use tree_sitter::{Node, Parser};

use super::site_types::Piece;
use super::values::UNREAD;

pub(crate) fn parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .expect("failed to load C# grammar");
    parser
}

pub(crate) fn text(node: Node, src: &[u8]) -> String {
    crate::parse::node_text(node, src)
}

/// The text with every whitespace character removed, the form a written
/// type name is compared in.
pub(crate) fn compact(node: Node, src: &[u8]) -> String {
    text(node, src)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

pub(crate) fn named<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub(crate) fn field<'t>(node: Node<'t>, name: &str) -> Option<Node<'t>> {
    node.child_by_field_name(name)
}

pub(crate) fn line(node: Node) -> usize {
    node.start_position().row + 1
}

/// Whether `path` is one or more identifiers joined by dots.
pub(crate) fn is_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('.').all(|seg| {
            let mut chars = seg.chars();
            chars
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '@')
                && chars.all(|c| c.is_alphanumeric() || c == '_')
        })
}

/// The content of a string literal node, or why it cannot be read; `None`
/// for a node that is no string literal at all.
pub(crate) fn string_literal(node: Node, src: &[u8]) -> Option<Result<String, &'static str>> {
    match node.kind() {
        "string_literal" => {
            let mut content = String::new();
            for part in named(node) {
                if part.kind() != "string_literal_content" {
                    return Some(Err(UNREAD));
                }
                content.push_str(&text(part, src));
            }
            Some(Ok(content))
        }
        "verbatim_string_literal" => {
            let raw = text(node, src);
            let inner = raw
                .strip_prefix("@\"")
                .and_then(|r| r.strip_suffix('"'))
                .map(|r| r.replace("\"\"", "\""));
            Some(inner.ok_or(UNREAD))
        }
        "raw_string_literal" => Some(Err(UNREAD)),
        _ => None,
    }
}

/// The pieces of a value written as string literals and field references
/// joined with `+`, or the reason word for any other form.
pub(crate) fn pieces(node: Node, src: &[u8]) -> Result<Vec<Piece>, &'static str> {
    if let Some(literal) = string_literal(node, src) {
        return literal.map(|t| vec![Piece::Lit(t)]);
    }
    match node.kind() {
        "binary_expression" => {
            let operator = field(node, "operator").map(|o| text(o, src));
            let (Some("+"), Some(left), Some(right)) = (
                operator.as_deref(),
                field(node, "left"),
                field(node, "right"),
            ) else {
                return Err("expression");
            };
            let mut all = pieces(left, src)?;
            all.extend(pieces(right, src)?);
            Ok(all)
        }
        "parenthesized_expression" => named(node)
            .first()
            .map_or(Err("expression"), |inner| pieces(*inner, src)),
        "identifier" | "member_access_expression" => {
            let path = compact(node, src);
            if is_path(&path) {
                Ok(vec![Piece::Ref(path)])
            } else {
                Err("expression")
            }
        }
        "interpolated_string_expression" => Err("interpolated-string"),
        "invocation_expression" => {
            let callee = field(node, "function").map(|f| text(f, src));
            Err(if callee.as_deref() == Some("nameof") {
                "nameof"
            } else {
                "method-call"
            })
        }
        _ => Err("expression"),
    }
}
