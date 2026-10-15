use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::text::{named_children, text};
use super::ts_fragment_types::TsFragmentDef;
use super::ts_purpose::{ts_declared_name, ts_pattern_names};

#[derive(Debug, Clone)]
pub(super) struct TsDecl {
    pub kind: &'static str,
    pub line: usize,
    pub end_line: usize,
}

pub(super) struct TsDecls {
    pub by_name: HashMap<String, TsDecl>,
    pub order: Vec<String>,
}

impl TsDecls {
    fn new() -> Self {
        Self {
            by_name: HashMap::new(),
            order: Vec::new(),
        }
    }

    fn add(&mut self, name: String, kind: &'static str, line: usize, end_line: usize) {
        if name.is_empty() || self.by_name.contains_key(&name) {
            return;
        }
        self.order.push(name.clone());
        self.by_name.insert(
            name,
            TsDecl {
                kind,
                line,
                end_line,
            },
        );
    }

    fn add_declaration(&mut self, node: Node, src: &[u8]) {
        let Some(kind) = ts_decl_kind(node) else {
            return;
        };
        if kind == "const" {
            for declarator in named_children(node) {
                if declarator.kind() != "variable_declarator"
                    || require_spec(declarator, src).is_some()
                {
                    continue;
                }
                let name_node = declarator.child_by_field_name("name");
                let line = name_node.unwrap_or(declarator).start_position().row + 1;
                for name in ts_pattern_names(name_node, src) {
                    self.add(name, kind, line, declarator.end_position().row + 1);
                }
            }
        } else {
            self.add(
                ts_declared_name(node, src),
                kind,
                node.start_position().row + 1,
                node.end_position().row + 1,
            );
        }
    }
}

pub(super) fn ts_decl_kind(node: Node) -> Option<&'static str> {
    match node.kind() {
        "class_declaration" | "abstract_class_declaration" => Some("class"),
        "function_declaration" | "generator_function_declaration" => Some("function"),
        "interface_declaration" => Some("interface"),
        "type_alias_declaration" => Some("type"),
        "enum_declaration" => Some("enum"),
        "lexical_declaration" | "variable_declaration" => Some("const"),
        _ => None,
    }
}

pub(super) fn require_spec(declarator: Node, src: &[u8]) -> Option<String> {
    let value = declarator.child_by_field_name("value")?;
    if value.kind() != "call_expression" {
        return None;
    }
    let function = value.child_by_field_name("function")?;
    if function.kind() != "identifier" || text(function, src) != "require" {
        return None;
    }
    let first = value.child_by_field_name("arguments")?.named_child(0)?;
    if first.kind() != "string" {
        return None;
    }
    Some(
        named_children(first)
            .into_iter()
            .find(|n| n.kind() == "string_fragment")
            .map(|n| text(n, src))
            .unwrap_or_default(),
    )
}

fn declaration_of(stmt: Node) -> Node {
    if stmt.kind() == "export_statement" {
        stmt.child_by_field_name("declaration").unwrap_or(stmt)
    } else {
        stmt
    }
}

fn iife_body(mut expr: Node) -> Option<Node> {
    if expr.kind() == "parenthesized_expression" {
        expr = expr.named_child(0)?;
    }
    if expr.kind() != "call_expression" {
        return None;
    }
    let mut callee = expr.child_by_field_name("function")?;
    if callee.kind() == "parenthesized_expression" {
        callee = callee.named_child(0)?;
    }
    if !matches!(callee.kind(), "function_expression" | "arrow_function") {
        return None;
    }
    let body = callee.child_by_field_name("body")?;
    (body.kind() == "statement_block").then_some(body)
}

fn statement_iife(stmt: Node) -> Option<Node> {
    let expr = stmt.named_child(0)?;
    if expr.kind() == "assignment_expression" {
        iife_body(expr.child_by_field_name("right")?)
    } else {
        iife_body(expr)
    }
}

fn bodies_at_level(container: Node) -> Vec<Node> {
    let mut bodies = Vec::new();
    for stmt in named_children(container) {
        if stmt.kind() == "expression_statement" {
            if let Some(body) = statement_iife(stmt) {
                bodies.push(body);
            }
        }
        let decl = declaration_of(stmt);
        if matches!(decl.kind(), "lexical_declaration" | "variable_declaration") {
            for d in named_children(decl) {
                if d.kind() == "variable_declarator" {
                    if let Some(body) = d.child_by_field_name("value").and_then(iife_body) {
                        bodies.push(body);
                    }
                }
            }
        }
    }
    bodies
}

pub(super) fn file_scope_decls(program: Node, src: &[u8]) -> (TsDecls, TsDecls) {
    let mut top = TsDecls::new();
    for stmt in named_children(program) {
        top.add_declaration(declaration_of(stmt), src);
    }
    let mut inside = TsDecls::new();
    let mut pending = bodies_at_level(program);
    while !pending.is_empty() {
        pending.sort_by_key(Node::start_byte);
        let body = pending.remove(0);
        for stmt in named_children(body) {
            inside.add_declaration(stmt, src);
        }
        pending.extend(bodies_at_level(body));
    }
    (top, inside)
}

pub(super) fn local_defs(
    top: &TsDecls,
    inside: &TsDecls,
    exported: &[String],
    imported: &HashSet<String>,
) -> Vec<TsFragmentDef> {
    let exported: HashSet<&str> = exported.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for (nested, table) in [(false, top), (true, inside)] {
        for name in &table.order {
            if exported.contains(name.as_str())
                || imported.contains(name)
                || (nested && top.by_name.contains_key(name))
            {
                continue;
            }
            let decl = &table.by_name[name];
            out.push(TsFragmentDef {
                name: name.clone(),
                kind: decl.kind.to_string(),
                line: decl.line,
                end_line: decl.end_line,
                local: true,
            });
        }
    }
    out
}
