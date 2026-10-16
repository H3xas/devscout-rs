//! Resolving a value to its evidence class: field references through the
//! repository's `const string` declarations, read from the declaring file
//! only for a field a kept site names.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;

use tree_sitter::{Node, Tree};

use super::catalogue::Val;
use super::fact_types::{FieldRef, ValueObj};
use super::qualify::{nested_id, Ctx, Qual};
use super::site_types::{Parsed, Part, RawValue};
use super::syntax::{
    compact, field, line, literal_at, named, parser, pieces, string_literal, text, Piece,
};
use super::values::{classify, is_name, UNREAD};

const MAX_DEPTH: usize = 8;

/// A resolved value: its evidence object, and the parts it states when it
/// was read.
#[derive(Debug, Clone)]
pub(crate) struct Resolved {
    pub(crate) obj: ValueObj,
    pub(crate) parts: Option<Parsed>,
    pub(crate) fields: Vec<FieldRef>,
}

impl Resolved {
    pub(crate) fn unknown(reason: &str) -> Resolved {
        Resolved {
            obj: ValueObj::Unknown {
                reason: reason.to_string(),
                key: None,
            },
            parts: None,
            fields: Vec::new(),
        }
    }
}

/// Where a reference is read from: its file, namespace and enclosing types.
#[derive(Debug, Clone)]
pub(crate) struct Place<'p> {
    pub(crate) file: &'p str,
    pub(crate) ns: &'p str,
    pub(crate) outer: &'p [String],
}

struct Source {
    tree: Tree,
    bytes: Vec<u8>,
}

pub(crate) struct Constants<'c, 'a> {
    ctx: &'c Ctx<'a>,
    sources: RefCell<HashMap<String, Option<Source>>>,
}

/// The value text a parsed value states for a position of kind `val`.
pub(crate) fn main_part(parsed: &Parsed, val: Val) -> Option<String> {
    match val {
        Val::HostName | Val::HostUri => parsed.host.clone(),
        Val::VirtualHost => parsed.virtual_host.clone(),
        Val::Region => parsed.region.clone(),
        _ => parsed.value.clone(),
    }
}

impl<'c, 'a> Constants<'c, 'a> {
    pub(crate) fn new(ctx: &'c Ctx<'a>) -> Self {
        Constants {
            ctx,
            sources: RefCell::new(HashMap::new()),
        }
    }

    /// Resolves a recorded value of kind `val` read at `place`.
    pub(crate) fn resolve(&self, raw: &RawValue, val: Val, place: &Place) -> Resolved {
        match raw {
            RawValue::Unknown { reason } => Resolved::unknown(reason),
            RawValue::Literal(parsed) => finish(parsed.clone(), val, Vec::new()),
            RawValue::Pieces { pieces } => {
                let pieces = match self.reread(pieces, place.file) {
                    Ok(pieces) => pieces,
                    Err(reason) => return Resolved::unknown(reason),
                };
                let mut fields = Vec::new();
                let mut seen = HashSet::new();
                match self.join(&pieces, place, 0, &mut seen, &mut fields) {
                    Ok(joined) => match classify(&joined, val) {
                        Ok(parsed) => finish(parsed, val, fields),
                        Err(reason) => Resolved::unknown(reason),
                    },
                    Err(reason) => Resolved::unknown(reason),
                }
            }
        }
    }

    // The pieces a cached value names, each literal read again at its
    // position in `file`. A literal that is gone, or that no longer reads
    // as a name, makes the value unknown.
    fn reread(&self, parts: &[Part], file: &str) -> Result<Vec<Piece>, &'static str> {
        let mut sources = self.sources.borrow_mut();
        let source = sources
            .entry(file.to_string())
            .or_insert_with(|| self.parse(file));
        let Some(source) = source else {
            return Err(UNREAD);
        };
        parts
            .iter()
            .map(|part| match part {
                Part::Ref(path) => Ok(Piece::Ref(path.clone())),
                Part::Lit(at) => {
                    let node = literal_at(source.tree.root_node(), *at).ok_or(UNREAD)?;
                    match string_literal(node, &source.bytes) {
                        Some(Ok(text)) if is_name(&text) => Ok(Piece::Lit(text, *at)),
                        _ => Err(UNREAD),
                    }
                }
            })
            .collect()
    }

    fn join(
        &self,
        all: &[Piece],
        place: &Place,
        depth: usize,
        seen: &mut HashSet<String>,
        fields: &mut Vec<FieldRef>,
    ) -> Result<String, &'static str> {
        let mut out = String::new();
        for piece in all {
            match piece {
                Piece::Lit(t, _) => out.push_str(t),
                Piece::Ref(path) => {
                    out.push_str(&self.reference(path, place, depth, seen, fields)?)
                }
            }
        }
        Ok(out)
    }

    // The type a reference's qualifier names, or the enclosing types for a
    // bare field name, as def indexes.
    fn owners(&self, qualifier: &str, place: &Place) -> Result<Vec<usize>, &'static str> {
        if qualifier.is_empty() {
            return Ok((1..=place.outer.len())
                .rev()
                .filter_map(|depth| {
                    let (types, name) = place.outer[..depth].split_at(depth - 1);
                    self.ctx.def_by_id(&nested_id(place.ns, types, &name[0]))
                })
                .collect());
        }
        match self
            .ctx
            .qualify(qualifier, place.file, place.ns, place.outer)
        {
            Qual::Repo(index) => Ok(vec![index]),
            Qual::Catalogue(_) => Err("package-constant"),
            Qual::Ambiguous => Err("unresolved-constant"),
            Qual::Nothing
                if self
                    .ctx
                    .declares_type(qualifier.rsplit('.').next().unwrap_or("")) =>
            {
                Err("unresolved-constant")
            }
            Qual::Nothing => Err("package-constant"),
        }
    }

    fn reference(
        &self,
        path: &str,
        place: &Place,
        depth: usize,
        seen: &mut HashSet<String>,
        fields: &mut Vec<FieldRef>,
    ) -> Result<String, &'static str> {
        if depth >= MAX_DEPTH {
            return Err("unresolved-constant");
        }
        let (qualifier, name) = path.rsplit_once('.').unwrap_or(("", path));
        for owner in self.owners(qualifier, place)? {
            let def = self.ctx.def(owner);
            let declared = self
                .ctx
                .graph
                .names
                .iter()
                .find(|n| n.owner == def.id && n.name == name && n.kind == "field");
            let Some(declared) = declared else { continue };
            let id = format!("{}.{name}", def.id);
            if !seen.insert(id.clone()) {
                return Err("unresolved-constant");
            }
            let initializer = self.const_initializer(&declared.file, declared.line, name)?;
            fields.push(FieldRef {
                field: id.clone(),
                file: declared.file.clone(),
                line: declared.line,
            });
            let outer = type_chain(def);
            let inner = Place {
                file: &declared.file,
                ns: &def.namespace,
                outer: &outer,
            };
            let value = self.join(&initializer, &inner, depth + 1, seen, fields);
            seen.remove(&id);
            return value;
        }
        Err("unresolved-constant")
    }

    // The initializer pieces of the `const string` field `name` declared at
    // `line` of `file`, or why the field is no constant.
    fn const_initializer(
        &self,
        file: &str,
        at: usize,
        name: &str,
    ) -> Result<Vec<Piece>, &'static str> {
        let mut sources = self.sources.borrow_mut();
        let source = sources
            .entry(file.to_string())
            .or_insert_with(|| self.parse(file));
        let Some(source) = source else {
            return Err("unresolved-constant");
        };
        let src = &source.bytes;
        let declarator =
            find_declarator(source.tree.root_node(), src, at, name).ok_or("unresolved-constant")?;
        let declaration = declarator.parent().ok_or("unresolved-constant")?;
        let holder = declaration.parent().ok_or("unresolved-constant")?;
        let modifiers: Vec<String> = named(holder)
            .into_iter()
            .filter(|c| c.kind() == "modifier")
            .map(|c| text(c, src))
            .collect();
        let has = |m: &str| modifiers.iter().any(|x| x == m);
        if !has("const") {
            return Err(if has("static") && has("readonly") {
                "static-readonly"
            } else {
                "field"
            });
        }
        let ty = field(declaration, "type").map(|t| compact(t, src));
        if !matches!(ty.as_deref(), Some("string" | "String" | "System.String")) {
            return Err("unresolved-constant");
        }
        let value = named(declarator)
            .into_iter()
            .rfind(|c| c.kind() != "identifier" || field(declarator, "name") != Some(*c))
            .ok_or("unresolved-constant")?;
        pieces(value, src)
    }

    fn parse(&self, file: &str) -> Option<Source> {
        let raw = fs::read_to_string(self.ctx.root.join(file)).ok()?;
        let stripped = crate::preproc::strip_inactive(&raw);
        let units = crate::parse::utf16_units(stripped.as_ref());
        let bytes = crate::parse::utf16_bytes(&units);
        let tree = parser().parse_utf16_le(&units, None)?;
        Some(Source { tree, bytes })
    }
}

fn finish(parsed: Parsed, val: Val, fields: Vec<FieldRef>) -> Resolved {
    let scheme = parsed.scheme.clone();
    let obj = match (val, main_part(&parsed, val)) {
        (Val::Key, Some(key)) => ValueObj::Configuration {
            key,
            values: Vec::new(),
        },
        (_, Some(value)) if fields.is_empty() => ValueObj::Literal { value, scheme },
        (_, Some(value)) => ValueObj::Constant {
            value,
            scheme,
            fields: fields.clone(),
        },
        (_, None) => ValueObj::NotStated,
    };
    Resolved {
        obj,
        parts: Some(parsed),
        fields,
    }
}

/// The enclosing type names of a def, outermost first, the def itself last.
pub(crate) fn type_chain(def: &crate::graph::Def) -> Vec<String> {
    let types = if def.namespace.is_empty() {
        def.id.as_str()
    } else {
        def.id
            .strip_prefix(&def.namespace)
            .and_then(|r| r.strip_prefix('.'))
            .unwrap_or(&def.id)
    };
    types.split('+').map(str::to_string).collect()
}

fn find_declarator<'t>(node: Node<'t>, src: &[u8], at: usize, name: &str) -> Option<Node<'t>> {
    if node.kind() == "variable_declarator"
        && line(node) == at
        && field(node, "name").is_some_and(|n| text(n, src) == name)
    {
        return Some(node);
    }
    let first = node.start_position().row + 1;
    let last = node.end_position().row + 1;
    if at < first || at > last {
        return None;
    }
    named(node)
        .into_iter()
        .find_map(|child| find_declarator(child, src, at, name))
}
