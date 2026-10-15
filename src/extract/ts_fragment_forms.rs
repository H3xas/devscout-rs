use tree_sitter::Node;

use super::text::{named_children, text};
use super::ts_fragment_types::{TsBinding, TsDynamicImport, TsImport};

pub(super) fn import_equals(stmt: Node, src: &[u8]) -> Option<TsImport> {
    let clause = named_children(stmt)
        .into_iter()
        .find(|child| child.kind() == "import_require_clause")?;
    let source = clause.child_by_field_name("source")?;
    let spec = string_literal(source, src)?;
    let local = named_children(clause)
        .into_iter()
        .find(|child| child.kind() == "identifier")?;
    Some(TsImport {
        spec,
        line: stmt.start_position().row + 1,
        bindings: vec![TsBinding {
            local: text(local, src),
            imported: "*".to_string(),
        }],
        require: true,
    })
}

pub(super) fn export_equals(stmt: Node, src: &[u8]) -> Option<String> {
    if stmt.kind() != "export_statement" || !text(stmt, src).starts_with("export =") {
        return None;
    }
    let value = stmt.child_by_field_name("value").or_else(|| {
        named_children(stmt)
            .into_iter()
            .find(|child| child.kind() == "identifier")
    })?;
    (value.kind() == "identifier").then(|| text(value, src))
}

pub(super) fn namespace_export(stmt: Node, src: &[u8]) -> Option<String> {
    let namespace = named_children(stmt)
        .into_iter()
        .find(|child| child.kind() == "namespace_export")?;
    let name = named_children(namespace)
        .into_iter()
        .find(|child| child.kind() == "identifier")?;
    Some(text(name, src))
}

pub(super) fn dynamic_import(call: Node, src: &[u8]) -> Option<Option<TsDynamicImport>> {
    let function = call.child_by_field_name("function")?;
    if function.kind() != "import" {
        return None;
    }
    let argument = call
        .child_by_field_name("arguments")
        .and_then(|args| named_children(args).into_iter().next());
    let spec = argument.and_then(|node| string_literal(node, src));
    Some(spec.map(|spec| TsDynamicImport {
        spec,
        line: call.start_position().row + 1,
    }))
}

fn string_literal(node: Node, src: &[u8]) -> Option<String> {
    match node.kind() {
        "string" | "template_string" => {
            let parts = named_children(node);
            if parts
                .iter()
                .any(|part| part.kind() == "template_substitution")
            {
                return None;
            }
            let literal = text(node, src);
            literal
                .get(1..literal.len().saturating_sub(1))
                .map(str::to_owned)
        }
        _ => None,
    }
}
