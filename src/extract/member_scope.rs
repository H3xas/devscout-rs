use std::collections::HashMap;

use tree_sitter::Node;

use super::qualifiers::MEMBER_SCOPE_NODES;
use super::receivers::{add_fact, FactTable};
use super::text::{named_children, text};
use super::types::Fact;

// ---------------------------------------------------------------------------
// Declaring constructs.
//
// Every member-table entry remembers the byte span of each construct that
// declares its name: the member itself for its own parameters, the enclosing
// block for a block local (a switch section's statement locals take the whole
// switch block), the lambda, anonymous method or local function for its
// parameters, the `foreach`, `catch` or query for its variables, and the
// nearest enclosing block or switch section for a pattern or `out`
// designation. A use outside every one of them cannot be vouched for by the
// table, so its lookup withholds instead of answering with a declaration the
// use never sees. The check only ever withholds: a use inside a declaring
// construct gets exactly the fact the table holds.
// ---------------------------------------------------------------------------

type Span = (usize, usize);

const WHOLE_MEMBER: Span = (0, usize::MAX);

fn span(node: Node) -> Option<Span> {
    Some((node.start_byte(), node.end_byte()))
}

pub(super) struct MemberTable {
    pub(super) facts: FactTable,
    constructs: HashMap<String, Vec<Span>>,
}

impl MemberTable {
    pub(super) fn new() -> Self {
        MemberTable {
            facts: FactTable::new(),
            constructs: HashMap::new(),
        }
    }

    // Enters `name` as declared by `decl`, with the fact the declaration
    // vouches for.
    pub(super) fn declare(&mut self, name: Option<String>, fact: Option<Fact>, decl: Node) {
        let Some(name) = name.filter(|n| !n.is_empty()) else {
            return;
        };
        let fact = if self.reserve(&name, decl) {
            fact
        } else {
            None
        };
        add_fact(&mut self.facts, Some(name), fact);
    }

    // Records the declaring construct of a name whose fact settles later.
    // `false` when the construct cannot be placed; the name is then already
    // taken-but-unknown, so whatever settles for it collapses to unknown.
    pub(super) fn reserve(&mut self, name: &str, decl: Node) -> bool {
        let placed = declaring_construct(decl);
        self.constructs
            .entry(name.to_string())
            .or_default()
            .push(placed.unwrap_or(WHOLE_MEMBER));
        if placed.is_none() {
            add_fact(&mut self.facts, Some(name.to_string()), None);
        }
        placed.is_some()
    }

    pub(super) fn settle(&mut self, name: &str, fact: Option<Fact>) {
        add_fact(&mut self.facts, Some(name.to_string()), fact);
    }

    pub(super) fn contains(&self, name: &str) -> bool {
        self.facts.contains_key(name)
    }

    // `true` when `name` is entered but no construct declaring it contains
    // the byte offset `at`.
    pub(super) fn withholds(&self, name: &str, at: usize) -> bool {
        self.facts.contains_key(name)
            && !self
                .constructs
                .get(name)
                .is_some_and(|spans| spans.iter().any(|&(s, e)| s <= at && at < e))
    }

    // `None` when `name` is not entered; `Some(None)` when it is taken but
    // nothing vouches for its type at `at`.
    pub(super) fn lookup(&self, name: &str, at: usize) -> Option<Option<Fact>> {
        let found = self.facts.get(name)?;
        if self.withholds(name, at) {
            return Some(None);
        }
        Some(found.clone())
    }
}

fn is_member(node: Node) -> bool {
    MEMBER_SCOPE_NODES.contains(&node.kind())
}

// `None` is a declaration whose construct the walk cannot classify: its name
// is taken for the whole member and vouches for no type there.
fn declaring_construct(decl: Node) -> Option<Span> {
    match decl.kind() {
        "parameter" => {
            let owner = decl.parent()?.parent()?;
            match owner.kind() {
                "lambda_expression"
                | "anonymous_method_expression"
                | "local_function_statement" => span(owner),
                _ if is_member(owner) => Some(WHOLE_MEMBER),
                _ => None,
            }
        }
        "implicit_parameter" => decl
            .parent()
            .filter(|p| p.kind() == "lambda_expression")
            .and_then(span),
        "variable_declaration" => {
            let parent = decl.parent()?;
            match parent.kind() {
                "for_statement" | "using_statement" | "fixed_statement" => span(parent),
                "local_declaration_statement" => statement_block(parent),
                _ => None,
            }
        }
        "foreach_statement" => span(decl),
        "catch_declaration" => decl
            .parent()
            .filter(|p| p.kind() == "catch_clause")
            .and_then(span),
        "from_clause" | "join_clause" | "join_into_clause" | "let_clause" | "query_expression" => {
            let mut node = Some(decl);
            while let Some(n) = node {
                if n.kind() == "query_expression" {
                    return span(n);
                }
                node = n.parent();
            }
            None
        }
        _ => designation_construct(decl),
    }
}

// A statement local is in scope for its whole block; a switch section's
// statement list is not a block of its own, so its locals span the switch body.
fn statement_block(statement: Node) -> Option<Span> {
    let mut node = statement.parent();
    while let Some(n) = node {
        match n.kind() {
            "block" => return span(n),
            "switch_section" => return n.parent().and_then(span),
            _ if is_member(n) => return None,
            _ => node = n.parent(),
        }
    }
    None
}

// A pattern or `out` designation: the nearest enclosing block or switch
// section, the `foreach` whose deconstruction it sits in, or the whole member
// when an expression body holds it.
fn designation_construct(decl: Node) -> Option<Span> {
    let mut child = decl;
    while let Some(n) = child.parent() {
        match n.kind() {
            "block" | "switch_section" => return span(n),
            "foreach_statement"
                if n.child_by_field_name("left")
                    .is_some_and(|l| l.id() == child.id()) =>
            {
                return span(n)
            }
            _ if is_member(n) => return Some(WHOLE_MEMBER),
            _ => child = n,
        }
    }
    Some(WHOLE_MEMBER)
}

// The names `node` declares that no type this walk reads vouches for: every
// name a `var (…)` deconstruction designates, in a declarator, a `foreach`, a
// pattern, or the misparsed `is var (…)`, and every name a LINQ query binds --
// in scope for the rest of the query, never a field of the enclosing type, and
// with no type readable off the syntax.
pub(super) fn untyped_declared_names(node: Node, src: &[u8]) -> Vec<String> {
    match node.kind() {
        "variable_declaration" => named_children(node)
            .into_iter()
            .filter(|d| {
                d.kind() == "variable_declarator" && d.child_by_field_name("name").is_none()
            })
            .flat_map(|d| designated_names(d, src))
            .collect(),
        "foreach_statement" => match node.child_by_field_name("left") {
            Some(l) if l.kind() == "tuple_pattern" => designated_names(l, src),
            Some(l) => misparsed_generic_variable(l, src).into_iter().collect(),
            None => Vec::new(),
        },
        "invocation_expression" => misparsed_var_pattern_names(node, src),
        "declaration_pattern" | "var_pattern" => pattern_designated_names(node, src),
        _ => query_binding_names(node, src),
    }
}

// Every name a `var (…)` deconstruction designates under `node`: the
// `tuple_pattern` of a declarator or `foreach`, or the
// `parenthesized_variable_designation` of a pattern, nested to any depth.
// Discards are their own node kind and never named.
fn designated_names(node: Node, src: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut cursor = node.walk();
    for named in node.children_by_field_name("name", &mut cursor) {
        names.push(text(named, src));
    }
    for c in named_children(node) {
        if matches!(
            c.kind(),
            "tuple_pattern" | "parenthesized_variable_designation"
        ) {
            names.extend(designated_names(c, src));
        }
    }
    names
}

// The grammar reads `foreach (Box<Item> x in xs)` -- a generic type whose
// argument is a name -- as the comparison `(Box < Item) > x` with no type node,
// so the loop variable is the comparison's right-hand identifier.
fn misparsed_generic_variable(left: Node, src: &[u8]) -> Option<String> {
    let is_op = |n: Node, op: &str| {
        n.kind() == "binary_expression"
            && n.child_by_field_name("operator")
                .is_some_and(|o| o.kind() == op)
    };
    let opened = left.child_by_field_name("left")?;
    let name = left.child_by_field_name("right")?;
    (is_op(left, ">") && is_op(opened, "<") && name.kind() == "identifier").then(|| text(name, src))
}

// The grammar has no rule for `e is var (x, y)` and reads it as an invocation
// of `e is var` with `x` and `y` as arguments. Invoking a type test is never
// valid C#, so that shape is always the deconstruction pattern; its names are
// the argument identifiers, nested through parenthesized tuples.
fn misparsed_var_pattern_names(node: Node, src: &[u8]) -> Vec<String> {
    let is_var_test = node
        .child_by_field_name("function")
        .filter(|f| f.kind() == "is_expression")
        .and_then(|f| f.child_by_field_name("right"))
        .is_some_and(|t| t.kind() == "implicit_type");
    match node.child_by_field_name("arguments") {
        Some(args) if is_var_test => argument_identifiers(args, src),
        _ => Vec::new(),
    }
}

fn argument_identifiers(list: Node, src: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for arg in named_children(list) {
        for c in named_children(arg) {
            match c.kind() {
                "identifier" if text(c, src) != "_" => names.push(text(c, src)),
                "tuple_expression" => names.extend(argument_identifiers(c, src)),
                _ => {}
            }
        }
    }
    names
}

// `case var (x, y):`, a `var (x, y)` switch-expression arm and a nested
// `var (x, y)` subpattern: the parenthesized designation of a pattern.
fn pattern_designated_names(node: Node, src: &[u8]) -> Vec<String> {
    named_children(node)
        .into_iter()
        .filter(|c| c.kind() == "parenthesized_variable_designation")
        .flat_map(|c| designated_names(c, src))
        .collect()
}

// The names one LINQ query clause BINDS -- `from d in xs`, `join o in ys`,
// `join ... into g`, `let n = ...`, and the query continuation `... into g`.
// Every one is a range variable: a name the rest of the query reads and the
// enclosing type never declared. Nothing here vouches for its TYPE (the
// element type of the source sequence is exactly what this extractor cannot
// compute), so the caller records each as taken-but-unknown -- which is the
// whole point: without an entry, the resolver's bare-identifier fallback
// would type `d` from a same-named field of the enclosing type or one of its
// bases and emit a precise edge the language never binds.
fn query_binding_names(n: Node, src: &[u8]) -> Vec<String> {
    match n.kind() {
        // `from [T] d in xs` -- the only clause that names its range
        // variable with a field.
        "from_clause" => n
            .child_by_field_name("name")
            .map(|x| text(x, src))
            .into_iter()
            .collect(),
        // `join [T] o in ys on a equals b`, `let n = ...`, `into g`: the
        // bound name is the FIRST bare identifier child that is not the
        // optional type node (a type can itself be an `identifier`).
        // Everything after it -- a `let`'s value, a join's source and its
        // two key expressions -- is an expression this must not claim.
        "join_clause" | "let_clause" | "join_into_clause" => {
            let type_id = n.child_by_field_name("type").map(|t| t.id());
            named_children(n)
                .into_iter()
                .find(|c| c.kind() == "identifier" && Some(c.id()) != type_id)
                .map(|c| text(c, src))
                .into_iter()
                .collect()
        }
        // The query continuation `... into g` has no node of its own -- the
        // grammar's query-body rule is hidden -- so its identifier sits as a
        // direct child of the `query_expression`, the one place a bare
        // identifier can appear there at all.
        "query_expression" => named_children(n)
            .into_iter()
            .filter(|c| c.kind() == "identifier")
            .map(|c| text(c, src))
            .collect(),
        _ => Vec::new(),
    }
}
