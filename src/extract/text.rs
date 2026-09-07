use std::collections::HashSet;

use tree_sitter::{Node, Parser};

pub(super) const MAX_PURPOSE: usize = 200;

// (grammar node kind, TYPE_KINDS label) pairs, iterated as an ordered slice
// to preserve declaration order.
const TYPE_KINDS: &[(&str, &str)] = &[
    ("class_declaration", "class"),
    ("interface_declaration", "interface"),
    ("struct_declaration", "struct"),
    ("record_declaration", "record"),
    ("enum_declaration", "enum"),
];

const NAMESPACE_NODES: &[&str] = &["namespace_declaration", "file_scoped_namespace_declaration"];

pub(super) fn type_kind_label(kind: &str) -> Option<&'static str> {
    TYPE_KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, label)| *label)
}

pub(super) fn new_parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .expect("failed to load C# grammar");
    parser
}

pub(super) fn named_children<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub(super) fn text(node: Node, src: &[u8]) -> String {
    crate::parse::node_text(node, src)
}

pub(super) fn declared_name(node: Node, src: &[u8]) -> String {
    node.child_by_field_name("name")
        .map(|n| text(n, src))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Purpose/signature composition.
// ---------------------------------------------------------------------------

pub(super) fn namespace_level_types<'a>(root: Node<'a>) -> Vec<Node<'a>> {
    let mut types = Vec::new();
    collect_types(root, &mut types);
    types
}

fn collect_types<'a>(node: Node<'a>, types: &mut Vec<Node<'a>>) {
    for child in named_children(node) {
        if NAMESPACE_NODES.contains(&child.kind()) {
            let body = child.child_by_field_name("body").unwrap_or(child);
            collect_types(body, types);
            continue;
        }
        if type_kind_label(child.kind()).is_some() {
            types.push(child);
        }
    }
}

fn base_list_text(node: Node, src: &[u8]) -> String {
    let Some(bases) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "base_list")
    else {
        return String::new();
    };
    let raw = text(bases, src);
    let after_colon = match raw.strip_prefix(':') {
        Some(rest) => rest.trim_start(),
        None => raw.as_str(),
    };
    collapse_whitespace(after_colon).trim().to_string()
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(ch);
            in_ws = false;
        }
    }
    out
}

pub(super) fn is_public(node: Node, src: &[u8]) -> bool {
    named_children(node)
        .into_iter()
        .any(|c| c.kind() == "modifier" && text(c, src) == "public")
}

fn strip_async_suffix(name: &str) -> String {
    name.strip_suffix("Async").unwrap_or(name).to_string()
}

// Purpose-signature method names: trailing "Async" stripped, deduped by the
// caller across the whole file. NOT the same helper as raw_method_names --
// a graph def needs the real, unabridged name (see that function's own
// comment).
fn public_method_names(node: Node, src: &[u8], kind: &str) -> Vec<String> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    named_children(body)
        .into_iter()
        .filter(|c| c.kind() == "method_declaration" && (kind == "interface" || is_public(*c, src)))
        .map(|c| strip_async_suffix(&declared_name(c, src)))
        .filter(|n| !n.is_empty())
        .collect()
}

// One namespace-level type's raw ingredients for a purpose segment --
// (line, kind, name, bases, unfiled public method names). Line comes along
// so `extract()`'s type-declaration-recovery path (below) can merge
// recovered types' parts into this list at the right sorted position
// before formatting; the ordinary (non-recovery) path never inspects it.
pub(super) type SegmentParts = (usize, &'static str, String, String, Vec<String>);

pub(super) fn type_segment_parts(node: Node, src: &[u8]) -> Option<SegmentParts> {
    let kind = type_kind_label(node.kind())?;
    let name = declared_name(node, src);
    if name.is_empty() {
        return None;
    }
    let bases = base_list_text(node, src);
    let methods = public_method_names(node, src, kind);
    Some((node.start_position().row + 1, kind, name, bases, methods))
}

// Formats an already line-ordered list of segment parts into one purpose
// string: "kind name[ : bases][; m1, m2]" joined with " | ", methods
// deduped globally across the whole list in order (a method name credited
// to an earlier type in the list is never repeated on a later one -- port
// of astPurposes' own cross-type dedup), then truncated to MAX_PURPOSE
// UTF-16 code units.
pub(super) fn format_segments(parts: Vec<SegmentParts>) -> Option<String> {
    let mut seen_methods: HashSet<String> = HashSet::new();
    let mut segments: Vec<String> = Vec::new();
    for (_line, kind, name, bases, raw_methods) in parts {
        let mut methods = Vec::new();
        for method in raw_methods {
            if seen_methods.contains(&method) {
                continue;
            }
            seen_methods.insert(method.clone());
            methods.push(method);
        }
        let header = if bases.is_empty() {
            format!("{kind} {name}")
        } else {
            format!("{kind} {name} : {bases}")
        };
        let segment = if methods.is_empty() {
            header
        } else {
            format!("{header}; {}", methods.join(", "))
        };
        segments.push(segment);
    }
    if segments.is_empty() {
        None
    } else {
        Some(truncate(&segments.join(" | ")))
    }
}

// NB: no `compose_signature(types, src)` wrapper -- extract()'s only
// caller needs to interleave a second (recovered) parts list before
// formatting, so it calls type_segment_parts()/format_segments() directly.

// The 200-unit limit counts UTF-16 code units, not bytes or chars -- matched
// here via `encode_utf16` rather than `str::len`/char indexing, so a purpose
// string with non-ASCII identifiers truncates at a code-unit boundary.
pub(super) fn truncate(s: &str) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    if units.len() > MAX_PURPOSE {
        let mut truncated = String::from_utf16_lossy(&units[..MAX_PURPOSE - 3]);
        truncated.push_str("...");
        truncated
    } else {
        s.to_string()
    }
}
