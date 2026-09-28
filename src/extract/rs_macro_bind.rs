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
//
// A statement or item macro may carry a binding from any nested group into
// its surrounding block or module. Generic parameters never do.
// ---------------------------------------------------------------------------

/// Names by what they may shadow.
#[derive(Default)]
pub(super) struct Names {
    /// Names that may be local bindings, which shadow a single-segment value path.
    pub values: HashSet<String>,
    /// Names that may be items, generic parameters or imports, which shadow
    /// a path's first segment.
    pub items: HashSet<String>,
}

/// What one invocation's token trees may bind.
#[derive(Default)]
pub(super) struct TreeNames {
    /// Everything the trees may bind, for the invocation's own calls.
    pub inside: Names,
    /// What any group in a statement or item invocation may bind around it.
    pub outward: Names,
    /// Whether the trees declare an `impl` or a trait.
    pub own_self: bool,
}

impl TreeNames {
    /// The context the invocation's own call sites are read in.
    pub fn narrow(self, ctx: &Ctx) -> Ctx {
        let mut inner = ctx.clone();
        inner.invocation_locals = Rc::new(self.inside.values);
        inner.invocation_shadow = Rc::new(self.inside.items);
        if self.own_self {
            inner.self_impl = None;
            inner.self_trait = None;
        }
        inner
    }
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
        let level = Level {
            tokens: &tokens,
            fields: struct_body(tree, src),
        };
        level.read(src, &mut names);
        stack.extend(tokens.iter().filter(|t| t.kind() == "token_tree").copied());
    }
    names
}

// One tree level: its direct tokens and whether it is a struct body.
struct Level<'a, 't> {
    tokens: &'a [Node<'t>],
    fields: bool,
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

impl Level<'_, '_> {
    // Reads the level's direct tokens, without descending. In a struct body
    // a name that opens a `field: value` entry names a field, not a
    // binding, unless an odd number of `|` before it puts it among closure
    // parameters.
    fn read(&self, src: &[u8], names: &mut TreeNames) {
        let tokens = self.tokens;
        let generics = generic_params(tokens);
        let mut angle: i64 = 0;
        let mut odd_pipes = false;
        for (i, token) in tokens.iter().enumerate() {
            match token.kind() {
                "impl" | "trait" => names.own_self = true,
                "use" => self.imports(i + 1, src, names),
                "<" => angle += 1,
                ">" => angle = (angle - 1).max(0),
                ">>" => angle = (angle - 2).max(0),
                "|" => odd_pipes = !odd_pipes,
                _ => {}
            }
            if !is_name(token.kind()) {
                continue;
            }
            if self.fields && !odd_pipes && opens_field(tokens, i) {
                continue;
            }
            let text = text_of(*token, src);
            if generics.contains(&i) {
                names.inside.items.insert(text);
            } else if declares_item(tokens, i) {
                self.add(names, text, false);
            } else if is_bare(tokens, i) && text != "Self" {
                if text.starts_with(|c: char| c.is_lowercase() || c == '_') {
                    self.add(names, text, true);
                } else if angle == 0 && !in_type_position(tokens, i) {
                    self.add(names, text, false);
                }
            }
        }
    }

    fn add(&self, names: &mut TreeNames, text: String, value: bool) {
        if value {
            names.inside.values.insert(text.clone());
        }
        names.inside.items.insert(text.clone());
        // The same token may expand to an item or a local. A nested group
        // can be spliced into either surrounding scope by the macro.
        names.outward.items.insert(text.clone());
        names.outward.values.insert(text);
    }

    // Every name a `use` declaration mentions, up to its `;` or the next
    // `use`, with its groups: each may be what it imports, in either
    // namespace.
    fn imports(&self, start: usize, src: &[u8], names: &mut TreeNames) {
        let mut stack: Vec<Node> = Vec::new();
        for token in self.tokens.iter().skip(start) {
            if matches!(token.kind(), ";" | "use") {
                break;
            }
            stack.push(*token);
        }
        while let Some(node) = stack.pop() {
            if is_name(node.kind()) {
                let text = text_of(node, src);
                self.add(names, text.clone(), true);
                self.add(names, text, false);
            }
            if node.kind() == "token_tree" {
                let mut cursor = node.walk();
                stack.extend(node.children(&mut cursor));
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
// list right after `impl`, or after an item keyword and its name. One pass
// with a stack of open lists, so nested or unclosed lists cost no rescan.
fn generic_params(tokens: &[Node]) -> HashSet<usize> {
    let mut out = HashSet::new();
    let mut open: Vec<bool> = Vec::new();
    for (k, node) in tokens.iter().enumerate() {
        match node.kind() {
            "<" => {
                let before = k.checked_sub(1).map(|p| tokens[p].kind());
                let after_item = k >= 2
                    && before.is_some_and(is_name)
                    && ITEM_KEYWORDS.contains(&tokens[k - 2].kind());
                open.push(before == Some("impl") || after_item);
            }
            ">" => {
                open.pop();
            }
            ">>" => {
                open.pop();
                open.pop();
            }
            kind if is_name(kind) => {
                let declares = open.last() == Some(&true)
                    && matches!(tokens[k - 1].kind(), "<" | "," | "const");
                if declares {
                    out.insert(k);
                }
            }
            _ => {}
        }
    }
    out
}

/// The index of the `>` or `>>` that closes each `<` of one level, found in
/// one pass: `>>` closes two.
pub(super) fn angle_closes(tokens: &[Node]) -> Vec<Option<usize>> {
    let mut closes = vec![None; tokens.len()];
    let mut open: Vec<usize> = Vec::new();
    for (k, node) in tokens.iter().enumerate() {
        let shut = match node.kind() {
            "<" => {
                open.push(k);
                0
            }
            ">" => 1,
            ">>" => 2,
            _ => 0,
        };
        for _ in 0..shut {
            if let Some(o) = open.pop() {
                closes[o] = Some(k);
            }
        }
    }
    closes
}
