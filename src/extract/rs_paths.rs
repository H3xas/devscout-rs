use tree_sitter::Node;

use crate::parse;

// ---------------------------------------------------------------------------
// Rust path nodes as segment lists. A path is walked down its left-nested
// `path` fields in a loop, so a long path costs no stack. Generic arguments
// are dropped wherever they sit; a leading `::` becomes an empty first
// segment, which joins back into the leading `::` as written.
// ---------------------------------------------------------------------------

/// One path segment: its text (without a raw-identifier prefix) and 1-based
/// line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Segment {
    pub text: String,
    pub line: usize,
}

/// The segments of a path node, or `None` for a path no name can reach: a
/// qualified path (`<T as Tr>::f`), a macro metavariable, or anything that is
/// not a path at all.
pub(super) fn path_segments(node: Node, src: &[u8]) -> Option<Vec<Segment>> {
    let mut out: Vec<Segment> = Vec::new();
    let mut current = node;
    loop {
        match current.kind() {
            "identifier" | "type_identifier" | "primitive_type" | "self" | "super" | "crate" => {
                out.push(segment(current, src));
                break;
            }
            "generic_type" | "generic_type_with_turbofish" => {
                current = current.child_by_field_name("type")?;
            }
            "scoped_identifier" | "scoped_type_identifier" => {
                let name = current.child_by_field_name("name")?;
                out.push(segment(name, src));
                match current.child_by_field_name("path") {
                    Some(path) => current = path,
                    None => {
                        out.push(Segment {
                            text: String::new(),
                            line: name.start_position().row + 1,
                        });
                        break;
                    }
                }
            }
            _ => return None,
        }
    }
    out.reverse();
    Some(out)
}

fn segment(node: Node, src: &[u8]) -> Segment {
    let text = parse::node_text(node, src);
    Segment {
        text: text.strip_prefix("r#").unwrap_or(&text).to_string(),
        line: node.start_position().row + 1,
    }
}

/// Segment texts joined by `::`.
pub(super) fn join(segments: &[Segment]) -> String {
    segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join("::")
}

/// The path node a type names, with references, raw pointers, `dyn` and
/// `impl` stripped, or `None` when the type has no single path (a tuple,
/// array, slice, function pointer, unit or `+`-bounded object).
pub(super) fn type_path_node(mut node: Node) -> Option<Node> {
    loop {
        let field = match node.kind() {
            "reference_type" | "pointer_type" => "type",
            "dynamic_type" | "abstract_type" => "trait",
            "type_identifier"
            | "scoped_type_identifier"
            | "generic_type"
            | "primitive_type"
            | "identifier"
            | "scoped_identifier" => return Some(node),
            _ => return None,
        };
        node = node.child_by_field_name(field)?;
    }
}
