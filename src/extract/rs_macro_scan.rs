use tree_sitter::Node;

use super::rs_facts_types::{RsRole, RsSite};
use super::rs_paths::Segment;
use super::rs_sites::{make_site, Ctx};
use crate::parse;

// ---------------------------------------------------------------------------
// Calls inside macro arguments. tree-sitter leaves a macro invocation's
// arguments as an unparsed token tree, so the calls in `assert_eq!`, `vec!`
// or `format!` are invisible to the site walk. This scan reads every token
// tree of an invocation, nested trees included, for `ident (` and
// `path :: ident (`, with an optional turbofish before the parenthesis.
//
// A name preceded by `.` (a method), `$` (a macro fragment) or `fn` (a
// definition) is no call, and neither is a nested macro's name (`inner!(`),
// whose own token tree is scanned in turn. String literals are single
// tokens, so nothing inside a format string is ever read. A hit is a `call`
// site marked `mac`, shadowed exactly like a call outside a macro. The scan
// is on an explicit stack: token trees nest without bound.
// ---------------------------------------------------------------------------

/// Adds a call site, keyed by its start byte, for every call the
/// invocation's token trees hold.
pub(super) fn scan(invocation: Node, src: &[u8], ctx: &Ctx, out: &mut Vec<(usize, RsSite)>) {
    let mut stack: Vec<Node> = Vec::new();
    let mut cursor = invocation.walk();
    for child in invocation.named_children(&mut cursor) {
        if child.kind() == "token_tree" {
            stack.push(child);
        }
    }
    while let Some(tree) = stack.pop() {
        scan_tree(tree, src, ctx, out);
        let mut cursor = tree.walk();
        for child in tree.named_children(&mut cursor) {
            if child.kind() == "token_tree" {
                stack.push(child);
            }
        }
    }
}

fn is_segment(node: Node) -> bool {
    matches!(node.kind(), "identifier" | "self" | "super" | "crate")
}

// Whether a path starts at `i`: a segment not continuing a path, or a
// leading `::` that follows no segment and no closing generic list.
fn starts_path(tokens: &[Node], i: usize) -> bool {
    let prev = i.checked_sub(1).map(|p| tokens[p].kind());
    if is_segment(tokens[i]) {
        return prev != Some("::");
    }
    tokens[i].kind() == "::"
        && tokens.get(i + 1).is_some_and(|n| is_segment(*n))
        && !prev
            .is_some_and(|p| matches!(p, ">" | ">>" | "identifier" | "self" | "super" | "crate"))
}

// One tree level: its direct tokens, without descending.
fn scan_tree(tree: Node, src: &[u8], ctx: &Ctx, out: &mut Vec<(usize, RsSite)>) {
    let mut cursor = tree.walk();
    let tokens: Vec<Node> = tree.children(&mut cursor).collect();
    let mut i = 0;
    while i < tokens.len() {
        if !starts_path(&tokens, i) {
            i += 1;
            continue;
        }
        let (segments, end) = read_path(&tokens, i, src);
        if !excluded_before(&tokens, i) {
            if let Some(site) = call_after(&tokens, end, &segments, ctx) {
                out.push((tokens[i].start_byte(), site));
            }
        }
        i = end.max(i + 1);
    }
}

// A path starting at `start`: segments joined by `::`, a leading `::` as an
// empty first segment, a turbofish inside the path skipped. Returns the
// segments and the index after the last segment.
fn read_path(tokens: &[Node], start: usize, src: &[u8]) -> (Vec<Segment>, usize) {
    let mut segments = Vec::new();
    let mut i = start;
    if tokens[i].kind() == "::" {
        segments.push(Segment {
            text: String::new(),
            line: tokens[i].start_position().row + 1,
        });
        i += 1;
    }
    loop {
        let text = parse::node_text(tokens[i], src);
        segments.push(Segment {
            text: text.strip_prefix("r#").unwrap_or(&text).to_string(),
            line: tokens[i].start_position().row + 1,
        });
        i += 1;
        let after = skip_turbofish(tokens, i);
        let continues = after + 1 < tokens.len()
            && tokens[after].kind() == "::"
            && is_segment(tokens[after + 1]);
        if !continues {
            return (segments, i);
        }
        i = after + 1;
    }
}

fn excluded_before(tokens: &[Node], start: usize) -> bool {
    start > 0 && matches!(tokens[start - 1].kind(), "." | "$" | "fn")
}

// A call site when the path is followed, past an optional turbofish, by a
// parenthesized token tree; `None` after `!` or anything else.
fn call_after(tokens: &[Node], end: usize, segments: &[Segment], ctx: &Ctx) -> Option<RsSite> {
    let next = skip_turbofish(tokens, end);
    let group = tokens.get(next)?;
    if group.kind() != "token_tree" || group.child(0).map(|c| c.kind()) != Some("(") {
        return None;
    }
    make_site(RsRole::Call, segments, ctx, true)
}

fn skip_turbofish(tokens: &[Node], at: usize) -> usize {
    if !(tokens.get(at).is_some_and(|n| n.kind() == "::")
        && tokens.get(at + 1).is_some_and(|n| n.kind() == "<"))
    {
        return at;
    }
    let mut depth: i64 = 0;
    for (k, node) in tokens.iter().enumerate().skip(at + 1) {
        depth += match node.kind() {
            "<" => 1,
            ">" => -1,
            ">>" => -2,
            _ => 0,
        };
        if depth <= 0 {
            return k + 1;
        }
    }
    at
}
