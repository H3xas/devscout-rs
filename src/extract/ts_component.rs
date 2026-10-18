// Angular component facts of one TS file: each top-level class with a
// `@Component({...})` decorator, its instance members, and its template --
// the literal `templateUrl` it names, or the scan of its inline `template`.
// The decorator is matched on the identifier `Component` alone; the class's
// import of it is not followed. Whether a class is exported is the graph's
// question: only an exported class has a def id for its members to name.

use serde::{Deserialize, Serialize};
use tree_sitter::Node;

use super::text::{named_children, text};
use crate::ngtemplate::{scan_template, GapReason, TemplateGap, TemplateScan};

/// One instance member a component class declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TsMember {
    /// The member name as written; a `#private` name keeps its `#`.
    pub name: String,
    /// `field`, `property` (a get or set accessor) or `method`.
    pub kind: String,
    /// The 1-based line of the first declaration of the name.
    pub line: usize,
}

/// A literal `templateUrl`, as written, with the line of its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TsTemplateUrl {
    /// The path as written, relative to the component file's directory.
    pub spec: String,
    /// The line of the string literal.
    pub line: usize,
}

/// One `@Component` class. Each list is omitted when empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TsComponent {
    /// The class name.
    pub name: String,
    /// The instance members, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<TsMember>,
    /// The external template the decorator names.
    #[serde(
        rename = "templateUrl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub template_url: Option<TsTemplateUrl>,
    /// The scan of the inline template, lines counted in this file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline: Option<TemplateScan>,
    /// A `templateUrl` or `template` whose value is not one plain literal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<TemplateGap>,
}

fn line_of(node: Node) -> usize {
    node.start_position().row + 1
}

fn has_token(node: Node, token: &str) -> bool {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).any(|c| c.kind() == token);
    found
}

/// Every `@Component` class among the program's top-level statements, in
/// source order.
pub(super) fn components(program: Node, src: &[u8]) -> Vec<TsComponent> {
    let mut out = Vec::new();
    for stmt in named_children(program) {
        let (class, mut decorators) = match stmt.kind() {
            "export_statement" => match stmt.child_by_field_name("declaration") {
                Some(decl) => (decl, field_nodes(stmt, "decorator")),
                None => continue,
            },
            _ => (stmt, Vec::new()),
        };
        if !matches!(
            class.kind(),
            "class_declaration" | "abstract_class_declaration"
        ) {
            continue;
        }
        decorators.extend(field_nodes(class, "decorator"));
        let Some(config) = decorators
            .into_iter()
            .find_map(|d| component_config(d, src))
        else {
            continue;
        };
        let Some(name) = class.child_by_field_name("name") else {
            continue;
        };
        let mut component = TsComponent {
            name: text(name, src),
            members: class
                .child_by_field_name("body")
                .map_or_else(Vec::new, |b| members(b, src)),
            template_url: None,
            inline: None,
            gaps: Vec::new(),
        };
        read_config(config, src, &mut component);
        out.push(component);
    }
    out
}

fn field_nodes<'t>(node: Node<'t>, field: &str) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    let nodes = node.children_by_field_name(field, &mut cursor).collect();
    nodes
}

// The object literal of a `@Component({...})` decorator.
fn component_config<'t>(decorator: Node<'t>, src: &[u8]) -> Option<Node<'t>> {
    let call = named_children(decorator)
        .into_iter()
        .find(|c| c.kind() == "call_expression")?;
    let callee = call.child_by_field_name("function")?;
    if callee.kind() != "identifier" || text(callee, src) != "Component" {
        return None;
    }
    let args = call.child_by_field_name("arguments")?;
    named_children(args)
        .into_iter()
        .next()
        .filter(|a| a.kind() == "object")
}

fn key_name(key: Node, src: &[u8]) -> Option<String> {
    match key.kind() {
        "property_identifier" | "private_property_identifier" => Some(text(key, src)),
        "string" => Some(
            named_children(key)
                .into_iter()
                .find(|c| c.kind() == "string_fragment")
                .map_or_else(String::new, |f| text(f, src)),
        ),
        _ => None,
    }
}

fn read_config(config: Node, src: &[u8], component: &mut TsComponent) {
    for entry in named_children(config) {
        let (key, value) = match entry.kind() {
            "pair" => {
                let Some(key) = entry
                    .child_by_field_name("key")
                    .and_then(|k| key_name(k, src))
                else {
                    continue;
                };
                (key, entry.child_by_field_name("value"))
            }
            "shorthand_property_identifier" => (text(entry, src), None),
            _ => continue,
        };
        let gap = |reason| TemplateGap {
            line: value.map_or(line_of(entry), line_of),
            reason,
        };
        let raw = value.and_then(|v| plain_literal(v, src));
        match (key.as_str(), raw) {
            ("templateUrl", Some((spec, line))) => {
                component.template_url = Some(TsTemplateUrl {
                    spec: cook(&spec),
                    line,
                });
            }
            ("templateUrl", None) => component.gaps.push(gap(GapReason::TemplateUrlNonLiteral)),
            ("template", Some((raw, line))) => {
                component.inline = Some(scan_template(&cook(&raw), line));
            }
            ("template", None) => component
                .gaps
                .push(gap(GapReason::InlineTemplateSubstitution)),
            _ => {}
        }
    }
}

// The raw text between the quotes of a single-quoted, double-quoted or
// backtick string with no `${}`, and the line its opening quote is on. Raw,
// so the template's lines stay aligned with the file's own.
fn plain_literal(value: Node, src: &[u8]) -> Option<(String, usize)> {
    let plain = match value.kind() {
        "string" => true,
        "template_string" => !named_children(value)
            .iter()
            .any(|c| c.kind() == "template_substitution"),
        _ => false,
    };
    if !plain {
        return None;
    }
    let whole = text(value, src);
    let inner = whole.get(1..whole.len().saturating_sub(1))?;
    Some((inner.to_string(), line_of(value)))
}

// Undoes the string escapes a template author can write without moving a
// line: an escaped newline character becomes a space, an escaped line break
// stays a line break, and any other escape becomes the character after the
// backslash.
fn cook(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'r' | 't') => out.push(' '),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

// The instance members of a class body, one row per name at its first
// declaration: a get and set pair is one property, and overload signatures
// join their method.
fn members(body: Node, src: &[u8]) -> Vec<TsMember> {
    let mut out: Vec<TsMember> = Vec::new();
    let mut add = |name: Option<String>, kind: &str, line: usize| {
        if let Some(name) = name.filter(|n| !n.is_empty()) {
            if !out.iter().any(|m| m.name == name) {
                out.push(TsMember {
                    name,
                    kind: kind.to_string(),
                    line,
                });
            }
        }
    };
    for member in named_children(body) {
        if has_token(member, "static") {
            continue;
        }
        let name_node = member.child_by_field_name("name");
        let name = name_node.and_then(|n| key_name(n, src));
        let line = name_node.map_or(line_of(member), line_of);
        match member.kind() {
            "public_field_definition" => add(name, "field", line),
            "method_definition" if name.as_deref() == Some("constructor") => {
                for (param, line) in parameter_properties(member, src) {
                    add(Some(param), "field", line);
                }
            }
            "method_definition" if has_token(member, "get") || has_token(member, "set") => {
                add(name, "property", line);
            }
            "method_definition" | "method_signature" | "abstract_method_signature" => {
                add(name, "method", line);
            }
            _ => {}
        }
    }
    out
}

// `constructor(private svc: Svc, readonly id: string)`: each parameter with
// an accessibility modifier or `readonly` declares a field.
fn parameter_properties(ctor: Node, src: &[u8]) -> Vec<(String, usize)> {
    let Some(params) = ctor.child_by_field_name("parameters") else {
        return Vec::new();
    };
    named_children(params)
        .into_iter()
        .filter(|p| {
            named_children(*p)
                .iter()
                .any(|c| c.kind() == "accessibility_modifier")
                || has_token(*p, "readonly")
        })
        .filter_map(|p| p.child_by_field_name("pattern"))
        .filter(|pattern| pattern.kind() == "identifier")
        .map(|pattern| (text(pattern, src), line_of(pattern)))
        .collect()
}
