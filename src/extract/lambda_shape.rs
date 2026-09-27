use tree_sitter::Node;

use super::refs::invocation_arguments;
use super::text::named_children;
use super::types::{LambdaShape, RefRecord};

// Stamps the member ref just pushed for an invocation window with the shape
// of each positional lambda literal among that window's own arguments, read
// from the same node `invocation_lambda_arg_arity` reads.
pub(super) fn stamp_lambda_arg_shape(refs: &mut [RefRecord], node: Node) {
    if let Some(last) = refs.last_mut() {
        last.lambda_arg_shape = invocation_lambda_arg_shape(node);
    }
}

fn invocation_lambda_arg_shape(node: Node) -> Option<Vec<Option<LambdaShape>>> {
    let shapes: Vec<Option<LambdaShape>> = invocation_arguments(node)?
        .into_iter()
        .map(lambda_argument_shape)
        .collect();
    shapes.iter().any(Option::is_some).then_some(shapes)
}

// A named argument can be reordered against the parameter list, so no
// position-bound fact is recorded for it, the same exclusion the arity fact
// applies. A local function passed as a method group and an anonymous method
// have no lambda body to read.
fn lambda_argument_shape(argument: Node) -> Option<LambdaShape> {
    if argument.child_by_field_name("name").is_some() {
        return None;
    }
    let lambda = named_children(argument)
        .into_iter()
        .find(|c| c.kind() == "lambda_expression")?;
    let body = lambda.child_by_field_name("body")?;
    let value_only = if body.kind() == "block" {
        block_returns_value(body)
    } else {
        expression_is_value_only(body)
    };
    Some(LambdaShape {
        value_only,
        implicitly_typed: parameters_are_implicitly_typed(lambda),
    })
}

fn parameters_are_implicitly_typed(lambda: Node) -> bool {
    match lambda.child_by_field_name("parameters") {
        Some(p) if p.kind() == "implicit_parameter" => true,
        Some(list) => named_children(list)
            .into_iter()
            .filter(|c| c.kind() == "parameter")
            .all(|p| p.child_by_field_name("type").is_none()),
        None => false,
    }
}

// C# converts an expression-bodied lambda to a void-returning delegate only
// when the body is a statement expression. The kinds listed here can never be
// one; every other kind, including the ones this list does not know, is
// treated as possibly void-compatible so the fact never refuses a conversion
// C# allows.
fn expression_is_value_only(body: Node) -> bool {
    match body.kind() {
        "boolean_literal"
        | "character_literal"
        | "integer_literal"
        | "null_literal"
        | "raw_string_literal"
        | "real_literal"
        | "string_literal"
        | "verbatim_string_literal"
        | "interpolated_string_expression"
        | "identifier"
        | "this"
        | "member_access_expression"
        | "element_access_expression"
        | "binary_expression"
        | "switch_expression"
        | "conditional_expression"
        | "is_expression"
        | "is_pattern_expression"
        | "as_expression"
        | "cast_expression"
        | "parenthesized_expression"
        | "default_expression"
        | "typeof_expression"
        | "lambda_expression" => true,
        "prefix_unary_expression" => !matches!(operator(body).as_deref(), Some("++" | "--")),
        "postfix_unary_expression" => operator(body).as_deref() == Some("!"),
        _ => false,
    }
}

fn operator(unary: Node) -> Option<String> {
    let mut cursor = unary.walk();
    let op = unary
        .children(&mut cursor)
        .find(|c| !c.is_named())
        .map(|c| c.kind().to_string());
    op
}

// A block body yields a value when it holds a `return <expr>;` of its own. A
// nested lambda, anonymous method or local function returns from itself, not
// from this lambda, so its statements are never searched.
fn block_returns_value(block: Node) -> bool {
    named_children(block)
        .into_iter()
        .any(|child| match child.kind() {
            "lambda_expression" | "anonymous_method_expression" | "local_function_statement" => {
                false
            }
            "return_statement" => named_children(child).iter().any(|c| c.kind() != "comment"),
            _ => block_returns_value(child),
        })
}
