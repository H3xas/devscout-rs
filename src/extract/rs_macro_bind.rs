use std::collections::HashSet;
use std::rc::Rc;

use tree_sitter::Node;

use super::rs_sites::Ctx;
use crate::parse;

// ---------------------------------------------------------------------------
// The names a macro invocation's token trees may bind. The trees are not
// parsed, and a macro can bind any identifier it is handed: a closure or
// `let` pattern inside `assert!`, a `match` arm inside `matches!`, a
// parameter of a function written inside an item-like macro, or a bare
// `$n:ident` expanded into `let $n = ...`. So every identifier outside a
// call or path position counts as a possible binding. That can drop a call
// that shares the name, but never binds a local to an item.
//
// Items, generic parameters and imports declared inside the trees shadow a
// path's first segment the same way, and an `impl` or a trait inside them
// makes `Self` name something other than the enclosing impl.
// ---------------------------------------------------------------------------

/// What one invocation's token trees may bind.
#[derive(Default)]
pub(super) struct TreeNames {
    /// Lowercase names that may be local bindings, which shadow a
    /// single-segment value path.
    pub values: HashSet<String>,
    /// Names that may be items, generic parameters or imports, which shadow
    /// a path's first segment.
    pub items: HashSet<String>,
    /// Whether the trees declare an `impl` or a trait.
    pub own_self: bool,
}

impl TreeNames {
    /// The context the invocation's own call sites are read in.
    pub fn narrow(self, ctx: &Ctx) -> Ctx {
        let mut inner = ctx.clone();
        inner.locals = union(&ctx.locals, self.values);
        inner.shadow = union(&ctx.shadow, self.items);
        if self.own_self {
            inner.self_impl = None;
            inner.self_trait = None;
        }
        inner
    }
}

fn union(base: &Rc<HashSet<String>>, more: HashSet<String>) -> Rc<HashSet<String>> {
    if more.iter().all(|n| base.contains(n)) {
        return Rc::clone(base);
    }
    let mut set = (**base).clone();
    set.extend(more);
    Rc::new(set)
}

const ITEM_KEYWORDS: &[&str] = &[
    "fn", "struct", "enum", "union", "trait", "type", "mod", "const", "static",
];

/// Whether an invocation stands as a statement of a block, where a binding
/// or an item it expands to is in scope for the rest of the block.
pub(super) fn is_statement(invocation: Node) -> bool {
    invocation
        .parent()
        .is_some_and(|p| matches!(p.kind(), "expression_statement" | "block"))
}

/// Every name the invocation's token trees, nested trees included, may
/// bind. The walk is on an explicit stack: token trees nest without bound.
pub(super) fn tree_names(invocation: Node, src: &[u8]) -> TreeNames {
    let mut names = TreeNames::default();
    let mut stack: Vec<Node> = Vec::new();
    let mut cursor = invocation.walk();
    for child in invocation.named_children(&mut cursor) {
        if child.kind() == "token_tree" {
            stack.push(child);
        }
    }
    while let Some(tree) = stack.pop() {
        let mut cursor = tree.walk();
        let tokens: Vec<Node> = tree.children(&mut cursor).collect();
        let fields = struct_body(tree, src);
        read_level(&tokens, fields, src, &mut names);
        stack.extend(tokens.iter().filter(|t| t.kind() == "token_tree"));
    }
    names
}

// Whether a tree is the braces of a struct literal or pattern: `{` right
// after an uppercase name (`Lamp { .. }`, `Kind::Wide { .. }`, `Self { .. }`).
fn struct_body(tree: Node, src: &[u8]) -> bool {
    let braced = tree.child(0).is_some_and(|c| c.kind() == "{");
    braced
        && tree.prev_sibling().is_some_and(|p| {
            is_name(p.kind()) && text_of(p, src).starts_with(|c: char| c.is_uppercase())
        })
}

fn is_name(kind: &str) -> bool {
    matches!(kind, "identifier" | "default" | "union" | "auto" | "raw")
}

fn text_of(node: Node, src: &[u8]) -> String {
    let text = parse::node_text(node, src);
    text.strip_prefix("r#").unwrap_or(&text).to_string()
}

// One tree level: its direct tokens, without descending. In a struct body a
// name that opens a `field: value` entry names a field, not a binding,
// unless an odd number of `|` before it puts it among closure parameters.
fn read_level(tokens: &[Node], fields: bool, src: &[u8], names: &mut TreeNames) {
    let generics = generic_params(tokens);
    let mut angle: i64 = 0;
    let mut pipes = 0usize;
    for (i, token) in tokens.iter().enumerate() {
        match token.kind() {
            "impl" | "trait" => names.own_self = true,
            "use" => import_names(tokens, i + 1, src, names),
            "<" => angle += 1,
            ">" => angle = (angle - 1).max(0),
            ">>" => angle = (angle - 2).max(0),
            "|" => pipes += 1,
            _ => {}
        }
        if !is_name(token.kind()) {
            continue;
        }
        if fields && pipes % 2 == 0 && opens_field(tokens, i) {
            continue;
        }
        let text = text_of(*token, src);
        if declares_item(tokens, i) || generics.contains(&i) {
            names.items.insert(text);
        } else if is_bare(tokens, i) && text != "Self" {
            if text.starts_with(|c: char| c.is_lowercase() || c == '_') {
                names.values.insert(text);
            } else if angle == 0 && !in_type_position(tokens, i) {
                names.items.insert(text);
            }
        }
    }
}

// An uppercase name inside angle brackets or after a type-position token is
// a type written in the tree, not a name the macro can declare. A lowercase
// name is never read this way: `<` is also a comparison, and a closure after
// one still binds.
fn in_type_position(tokens: &[Node], i: usize) -> bool {
    i.checked_sub(1).is_some_and(|p| {
        matches!(
            tokens[p].kind(),
            ":" | "->" | "as" | "&" | "dyn" | "impl" | "for" | "mutable_specifier"
        )
    })
}

// A name at the start of a struct body entry, before its `:`.
fn opens_field(tokens: &[Node], i: usize) -> bool {
    let after_start = i > 0 && matches!(tokens[i - 1].kind(), "{" | ",");
    after_start && tokens.get(i + 1).is_some_and(|n| n.kind() == ":")
}

// A name right after an item keyword (`fn f`, `static mut S`).
fn declares_item(tokens: &[Node], i: usize) -> bool {
    let kind = |k: usize| tokens[k].kind();
    match i.checked_sub(1) {
        Some(p) if ITEM_KEYWORDS.contains(&kind(p)) => true,
        Some(p) if kind(p) == "mutable_specifier" => p > 0 && kind(p - 1) == "static",
        _ => false,
    }
}

// A name outside a call or path position: not after `.`, `::`, `$` or a
// lifetime's quote, and not before `::`, `!` or a parenthesized tree.
fn is_bare(tokens: &[Node], i: usize) -> bool {
    let prev = i.checked_sub(1).map(|p| tokens[p].kind());
    if prev.is_some_and(|p| matches!(p, "." | "::" | "$" | "'")) {
        return false;
    }
    match tokens.get(i + 1) {
        Some(next) if matches!(next.kind(), "::" | "!") => false,
        Some(next) if next.kind() == "token_tree" => next.child(0).map(|c| c.kind()) != Some("("),
        _ => true,
    }
}

// The token indexes of the names a generic parameter list declares: the
// list right after `impl`, or after an item keyword and its name.
fn generic_params(tokens: &[Node]) -> HashSet<usize> {
    let mut out = HashSet::new();
    for (open, token) in tokens.iter().enumerate() {
        if token.kind() != "<" || open == 0 {
            continue;
        }
        let before = tokens[open - 1].kind();
        let after_item =
            open >= 2 && is_name(before) && ITEM_KEYWORDS.contains(&tokens[open - 2].kind());
        if before != "impl" && !after_item {
            continue;
        }
        let mut depth: i64 = 0;
        for (k, node) in tokens.iter().enumerate().skip(open) {
            depth += match node.kind() {
                "<" => 1,
                ">" => -1,
                ">>" => -2,
                _ => 0,
            };
            if depth <= 0 {
                break;
            }
            let declares = depth == 1
                && is_name(node.kind())
                && matches!(tokens[k - 1].kind(), "<" | "," | "const");
            if declares {
                out.insert(k);
            }
        }
    }
    out
}

// Every name a `use` declaration inside the trees mentions, up to its `;`,
// with its groups: each may be what it imports, in either namespace.
fn import_names(tokens: &[Node], start: usize, src: &[u8], names: &mut TreeNames) {
    let mut stack: Vec<Node> = Vec::new();
    for token in tokens.iter().skip(start) {
        if token.kind() == ";" {
            break;
        }
        stack.push(*token);
    }
    while let Some(node) = stack.pop() {
        if is_name(node.kind()) {
            let text = text_of(node, src);
            names.values.insert(text.clone());
            names.items.insert(text);
        }
        if node.kind() == "token_tree" {
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
    }
}
