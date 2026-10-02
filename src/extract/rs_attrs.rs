use tree_sitter::Node;

use super::rs_paths::path_segments;
use crate::parse;

// ---------------------------------------------------------------------------
// The outer attributes of a Rust item, read in source order, with each
// `cfg_attr(<predicate>, <attr>, ...)` standing for the attributes it
// carries. The predicate is never evaluated, the same as every other `cfg`
// in the Rust model, so a carried attribute counts whatever the predicate
// says. A `cfg_attr` whose argument list is not a predicate followed by at
// least one attribute stands for nothing.
//
// Inside a `cfg_attr` only an exact attribute form is read: a path, then
// nothing, one delimited token tree, or `= <one literal>`. Nested `cfg_attr`
// lists unwind on an explicit stack, because their depth is unbounded in
// hostile input and extraction runs on worker threads with small stacks.
// ---------------------------------------------------------------------------

/// What a `mod x;` declaration's attributes say of its file: `None` when
/// no attribute names one, `Some(Some(literal))` for a plain `#[path]` with
/// a plain string literal, and `Some(None)` when the file cannot be told.
pub(super) type PathFact = Option<Option<String>>;

// One attribute as read: its path, what follows the path, and whether a
// `cfg_attr` carried it.
struct Attr {
    path: Vec<String>,
    form: Form,
    carried: bool,
}

enum Form {
    Bare,
    Args,
    // `= value`: the value when it is a plain string literal.
    Value(Option<String>),
    Malformed,
}

/// Whether an outer attribute of the item is `#[test]` or a path ending in
/// `::test` (`#[tokio::test]`), plainly or carried by `cfg_attr`.
pub(super) fn is_test(node: Node, src: &[u8]) -> bool {
    attributes(node, src).iter().any(|a| {
        a.path.last().is_some_and(|s| s == "test")
            && (!a.carried || matches!(a.form, Form::Bare | Form::Args))
    })
}

/// The first outer `path` attribute's fact for a module declaration. A
/// `path` carried by `cfg_attr` is unread: which file it names depends on
/// the predicate.
pub(super) fn path_fact(node: Node, src: &[u8]) -> PathFact {
    let attr = attributes(node, src)
        .into_iter()
        .find(|a| a.path.len() == 1 && a.path[0] == "path")?;
    match attr.form {
        Form::Value(Some(literal)) if !attr.carried => Some(Some(literal)),
        _ => Some(None),
    }
}

// Every outer attribute directly above the item, in source order, with
// each `cfg_attr` expanded in place.
fn attributes(node: Node, src: &[u8]) -> Vec<Attr> {
    let mut items = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(prev) = current {
        match prev.kind() {
            "attribute_item" => items.extend(prev.named_child(0)),
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        current = prev.prev_sibling();
    }
    items.reverse();
    let mut out = Vec::new();
    for attribute in items {
        plain(attribute, src, &mut out);
    }
    out
}

// A plain `#[...]` attribute: its path is a path node, then an `arguments`
// token tree or a `value` expression.
fn plain(attribute: Node, src: &[u8], out: &mut Vec<Attr>) {
    let Some(path) = attribute
        .named_child(0)
        .and_then(|p| path_segments(p, src))
        .map(|segs| segs.into_iter().map(|s| s.text).collect::<Vec<_>>())
    else {
        return;
    };
    let args = attribute.child_by_field_name("arguments");
    if path == ["cfg_attr"] {
        if let Some(tree) = args {
            expand(tree, src, out);
        }
        return;
    }
    let form = if args.is_some() {
        Form::Args
    } else if let Some(value) = attribute.child_by_field_name("value") {
        Form::Value(string_literal(value, src))
    } else {
        Form::Bare
    };
    out.push(Attr {
        path,
        form,
        carried: false,
    });
}

// Expands one `cfg_attr` argument tree, and every `cfg_attr` it carries,
// into `out` in source order.
fn expand(tree: Node, src: &[u8], out: &mut Vec<Attr>) {
    let mut stack = vec![tree];
    let mut found = Vec::new();
    while let Some(tree) = stack.pop() {
        let Some(parts) = split(tree) else {
            continue;
        };
        for part in parts.into_iter().skip(1).rev() {
            let attr = read_carried(&part, src);
            match (&attr.form, part.last()) {
                (Form::Args, Some(inner)) if attr.path == ["cfg_attr"] => stack.push(*inner),
                _ => found.push((part[0].start_byte(), attr)),
            }
        }
    }
    found.sort_by_key(|(start, _)| *start);
    out.extend(found.into_iter().map(|(_, a)| a));
}

// The argument tree's tokens split at its top-level commas: a predicate and
// one or more attributes, a trailing comma allowed. `None` for any other
// shape.
fn split(tree: Node) -> Option<Vec<Vec<Node>>> {
    let mut cursor = tree.walk();
    let tokens: Vec<Node> = tree.children(&mut cursor).collect();
    if tokens.len() < 2 || tokens[0].kind() != "(" || tokens[tokens.len() - 1].kind() != ")" {
        return None;
    }
    let mut parts: Vec<Vec<Node>> = vec![Vec::new()];
    for token in &tokens[1..tokens.len() - 1] {
        if token.kind() == "," {
            parts.push(Vec::new());
        } else {
            parts.last_mut()?.push(*token);
        }
    }
    if parts.len() > 2 && parts.last().is_some_and(Vec::is_empty) {
        parts.pop();
    }
    (parts.len() >= 2 && parts.iter().all(|p| !p.is_empty())).then_some(parts)
}

// One carried attribute from its tokens.
fn read_carried(tokens: &[Node], src: &[u8]) -> Attr {
    let mut path = Vec::new();
    let mut k = usize::from(tokens.first().is_some_and(|t| t.kind() == "::"));
    while let Some(token) = tokens.get(k).filter(|t| t.kind() == "identifier") {
        path.push(parse::node_text(*token, src));
        if tokens.get(k + 1).is_some_and(|t| t.kind() == "::") {
            k += 2;
        } else {
            k += 1;
            break;
        }
    }
    let rest = tokens.get(k..).unwrap_or_default();
    let form = match rest {
        _ if path.is_empty() => Form::Malformed,
        [] => Form::Bare,
        [tree] if tree.kind() == "token_tree" => Form::Args,
        [eq, value] if eq.kind() == "=" => Form::Value(string_literal(*value, src)),
        _ => Form::Malformed,
    };
    Attr {
        path,
        form,
        carried: true,
    }
}

// The value of a plain `"..."` literal with no escape sequence, or of a raw
// `r"..."` literal; `None` for any other value, including byte and C
// strings.
fn string_literal(node: Node, src: &[u8]) -> Option<String> {
    let text = parse::node_text(node, src);
    match node.kind() {
        "string_literal" => {
            let mut cursor = node.walk();
            let escaped = node
                .named_children(&mut cursor)
                .any(|c| c.kind() == "escape_sequence");
            let inner = text.strip_prefix('"')?.strip_suffix('"')?;
            (!escaped).then(|| inner.to_string())
        }
        "raw_string_literal" => {
            let body = text.strip_prefix('r')?;
            let hashes = body.len() - body.trim_start_matches('#').len();
            let close = format!("\"{}", "#".repeat(hashes));
            body[hashes..]
                .strip_prefix('"')?
                .strip_suffix(close.as_str())
                .map(str::to_string)
        }
        _ => None,
    }
}
