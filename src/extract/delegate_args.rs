use tree_sitter::Node;

use super::text::{named_children, text};

// The parameter count of one call argument whose delegate parameter list
// the syntax alone settles: a lambda literal (`x => ...`, `(a, b) => ...`,
// counted from its own `parameters` field -- 1 for the bare
// `implicit_parameter` shape, else the parenthesized list's length, 0 for
// `() => ...`), or a bare identifier naming a local function in scope,
// which converts as a method group whose one signature is right there in
// the enclosing member body. `None` for a named argument (`configure: x =>
// ...`, the same exclusion `lambda_argument_slot` applies, for the same
// reason -- a positional delegate-parameter comparison is not safe once
// positions can be reordered) and for every other argument: a method group
// naming a member of some type needs overload resolution to know which
// signature converts, so it records nothing rather than a guess.
pub(super) fn delegate_argument_param_count(argument: Node, src: &[u8]) -> Option<usize> {
    if argument.child_by_field_name("name").is_some() {
        return None;
    }
    let children = named_children(argument);
    if let Some(lambda) = children.iter().find(|c| c.kind() == "lambda_expression") {
        let params = lambda.child_by_field_name("parameters")?;
        return Some(if params.kind() == "implicit_parameter" {
            1
        } else {
            named_children(params).len()
        });
    }
    match children.as_slice() {
        [identifier] if identifier.kind() == "identifier" && argument.child_count() == 1 => {
            local_function_param_count(*identifier, src)
        }
        _ => None,
    }
}

// C# simple-name lookup for `identifier`, restricted to local functions: a
// local function is visible throughout the block that declares it, so walk
// outward through every enclosing block up to the type member, collecting
// same-named local functions. Answers only when exactly one is in scope and
// nothing nearer rebinds the name -- a lambda, anonymous-method or
// local-function parameter, a local variable, or a `foreach` variable, all
// of which a nested function may declare under an outer local function's
// name. Anything else is a different method group and fails open.
fn local_function_param_count(identifier: Node, src: &[u8]) -> Option<usize> {
    let name = text(identifier, src);
    let mut declared = Vec::new();
    let mut node = identifier.parent();
    while let Some(scope) = node {
        match scope.kind() {
            "block" => {
                for statement in named_children(scope) {
                    match statement.kind() {
                        "local_function_statement" if name_is(statement, &name, src) => {
                            declared.push(statement);
                        }
                        "local_declaration_statement" if declares_local(statement, &name, src) => {
                            return None;
                        }
                        _ => {}
                    }
                }
            }
            "lambda_expression" | "anonymous_method_expression" | "local_function_statement" => {
                if parameters(scope)
                    .iter()
                    .any(|p| parameter_is(*p, &name, src))
                {
                    return None;
                }
            }
            "foreach_statement" => {
                if scope
                    .child_by_field_name("left")
                    .is_some_and(|left| text(left, src) == name)
                {
                    return None;
                }
            }
            "declaration_list" | "compilation_unit" | "global_statement" => break,
            _ => {}
        }
        node = scope.parent();
    }
    match declared.as_slice() {
        [function] => Some(parameters(*function).len()),
        _ => None,
    }
}

fn name_is(node: Node, name: &str, src: &[u8]) -> bool {
    node.child_by_field_name("name")
        .is_some_and(|n| text(n, src) == name)
}

fn parameter_is(parameter: Node, name: &str, src: &[u8]) -> bool {
    if parameter.kind() == "implicit_parameter" {
        text(parameter, src) == name
    } else {
        name_is(parameter, name, src)
    }
}

// The declared parameters of a lambda, anonymous method or local function:
// the bare `implicit_parameter` itself, or each `parameter` of its list.
fn parameters(function: Node) -> Vec<Node> {
    match function.child_by_field_name("parameters") {
        Some(p) if p.kind() == "implicit_parameter" => vec![p],
        Some(list) => named_children(list)
            .into_iter()
            .filter(|c| c.kind() == "parameter")
            .collect(),
        None => Vec::new(),
    }
}

fn declares_local(statement: Node, name: &str, src: &[u8]) -> bool {
    named_children(statement)
        .into_iter()
        .filter(|c| c.kind() == "variable_declaration")
        .flat_map(named_children)
        .any(|d| d.kind() == "variable_declarator" && name_is(d, name, src))
}
