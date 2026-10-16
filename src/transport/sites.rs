//! The per-file syntax walk that records provisional transport sites.
//!
//! The walk keeps a scope of the names it can type: lambda parameters that
//! a catalogue signature types, and declared fields, properties, parameters
//! and locals with the type as written. A call is a site only when its
//! receiver is one of those, a static catalogue type, the return value of
//! another site, or, for a chain start, any receiver at all. Nothing here
//! decides that a written name really is the catalogue type it looks like;
//! the assembly stage does, with the project's usings in hand.

use std::sync::OnceLock;

use tree_sitter::Node;

use super::catalogue::{self, Form, Ty, ATTRIBUTES, DEFINITIONS};
use super::site_types::{RawValue, SiteType, SiteValue, TransportSite};
use super::syntax::{compact, field, line, named, text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    Param,
    Local,
    Field,
}

#[derive(Debug, Clone)]
pub(super) enum VarTy {
    /// Typed by a catalogue signature: the type and the site that types it.
    Cat(&'static str, usize),
    /// Declared with this type as written.
    Written(String),
    Untyped,
}

#[derive(Debug, Clone)]
pub(super) struct Var {
    pub(super) origin: Origin,
    pub(super) ty: VarTy,
}

/// What a call's receiver is, as far as the walk can say.
#[derive(Debug, Clone)]
pub(super) enum Recv {
    /// A catalogue type: through a site (`parent`), or through a written
    /// name the assembly stage must qualify (`written`).
    Typed {
        ty: &'static str,
        parent: Option<usize>,
        written: Option<String>,
    },
    /// A declared type outside the catalogue, as written.
    Known(String),
    /// An expression the walk cannot type.
    Untyped,
    /// No receiver at all, outside a definition.
    Bare,
}

pub(super) struct Walk<'s> {
    pub(super) src: &'s [u8],
    pub(super) sites: Vec<TransportSite>,
    ns: String,
    outer: Vec<String>,
    frames: Vec<Vec<(String, Var)>>,
    def: Option<(usize, &'static str)>,
}

const TYPE_DECLS: [&str; 5] = [
    "class_declaration",
    "struct_declaration",
    "record_declaration",
    "record_struct_declaration",
    "interface_declaration",
];

const MEMBER_DECLS: [&str; 7] = [
    "method_declaration",
    "constructor_declaration",
    "local_function_statement",
    "operator_declaration",
    "conversion_operator_declaration",
    "destructor_declaration",
    "indexer_declaration",
];

fn prefilter() -> &'static [&'static str] {
    static WORDS: OnceLock<Vec<&'static str>> = OnceLock::new();
    WORDS.get_or_init(catalogue::prefilter_words)
}

/// The provisional sites of one parsed C# file. `source` is the text the
/// tree was parsed from, read only by the prefilter.
pub fn sites(source: &str, root: Node, src: &[u8]) -> Vec<TransportSite> {
    if !prefilter().iter().any(|w| source.contains(w)) {
        return Vec::new();
    }
    let mut walk = Walk {
        src,
        sites: Vec::new(),
        ns: String::new(),
        outer: Vec::new(),
        frames: vec![Vec::new()],
        def: None,
    };
    walk.children(root);
    walk.sites
}

impl<'s> Walk<'s> {
    pub(super) fn text(&self, node: Node) -> String {
        text(node, self.src)
    }

    pub(super) fn compact(&self, node: Node) -> String {
        compact(node, self.src)
    }

    pub(super) fn children(&mut self, node: Node) {
        for child in named(node) {
            if child.kind() == "file_scoped_namespace_declaration" {
                let name = field(child, "name")
                    .map(|n| self.compact(n))
                    .unwrap_or_default();
                self.ns = join(&self.ns, &name);
                continue;
            }
            self.visit(child);
        }
    }

    pub(super) fn visit(&mut self, node: Node) {
        let kind = node.kind();
        if TYPE_DECLS.contains(&kind) {
            return self.type_decl(node);
        }
        if MEMBER_DECLS.contains(&kind) {
            return self.member(node);
        }
        match kind {
            "namespace_declaration" => self.namespace(node),
            "lambda_expression" | "anonymous_method_expression" => self.lambda(node, None),
            "block" | "switch_section" => self.scoped(|w| w.children(node)),
            "local_declaration_statement" => self.local(node),
            "foreach_statement" => self.foreach(node),
            "invocation_expression" => {
                self.invocation(node);
            }
            "assignment_expression" => self.assignment(node),
            "member_access_expression" => {
                self.expr(node);
            }
            _ => self.children(node),
        }
    }

    fn scoped(&mut self, body: impl FnOnce(&mut Self)) {
        self.frames.push(Vec::new());
        body(self);
        self.frames.pop();
    }

    pub(super) fn bind(&mut self, name: String, var: Var) {
        if let Some(frame) = self.frames.last_mut() {
            frame.push((name, var));
        }
    }

    pub(super) fn lookup(&self, name: &str) -> Option<&Var> {
        self.frames
            .iter()
            .rev()
            .flat_map(|f| f.iter().rev())
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    fn namespace(&mut self, node: Node) {
        let name = field(node, "name")
            .map(|n| self.compact(n))
            .unwrap_or_default();
        let saved = std::mem::take(&mut self.ns);
        self.ns = join(&saved, &name);
        if let Some(body) = field(node, "body") {
            self.children(body);
        }
        self.ns = saved;
    }

    fn type_decl(&mut self, node: Node) {
        let name = field(node, "name")
            .map(|n| self.text(n))
            .unwrap_or_default();
        let def = self.definition_site(node, &name);
        self.attribute_sites(node, &name);
        self.outer.push(name);
        let saved_def = std::mem::replace(&mut self.def, def);
        self.frames.push(Vec::new());
        if let Some(params) = named(node)
            .into_iter()
            .find(|c| c.kind() == "parameter_list")
        {
            self.bind_params(params, Origin::Param);
        }
        if let Some(body) = field(node, "body") {
            self.bind_members(body);
            self.children(body);
        }
        self.frames.pop();
        self.def = saved_def;
        self.outer.pop();
    }

    fn bind_members(&mut self, body: Node) {
        for member in named(body) {
            match member.kind() {
                "field_declaration" | "event_field_declaration" => {
                    if let Some(decl) = named(member)
                        .into_iter()
                        .find(|c| c.kind() == "variable_declaration")
                    {
                        self.bind_declaration(decl, Origin::Field);
                    }
                }
                "property_declaration" => {
                    let ty = field(member, "type").map(|t| self.compact(t));
                    if let (Some(ty), Some(name)) = (ty, field(member, "name")) {
                        let var = Var {
                            origin: Origin::Field,
                            ty: VarTy::Written(ty),
                        };
                        self.bind(self.text(name), var);
                    }
                }
                _ => {}
            }
        }
    }

    fn bind_declaration(&mut self, decl: Node, origin: Origin) {
        let ty = field(decl, "type");
        let var_ty = match ty {
            Some(t) if t.kind() != "implicit_type" => VarTy::Written(self.compact(t)),
            _ => VarTy::Untyped,
        };
        for declarator in named(decl)
            .into_iter()
            .filter(|c| c.kind() == "variable_declarator")
        {
            if let Some(name) = field(declarator, "name") {
                let var = Var {
                    origin,
                    ty: var_ty.clone(),
                };
                self.bind(self.text(name), var);
            }
        }
    }

    fn bind_params(&mut self, params: Node, origin: Origin) {
        for param in named(params)
            .into_iter()
            .filter(|c| c.kind() == "parameter")
        {
            let ty =
                field(param, "type").map_or(VarTy::Untyped, |t| VarTy::Written(self.compact(t)));
            if let Some(name) = field(param, "name") {
                self.bind(self.text(name), Var { origin, ty });
            }
        }
    }

    fn member(&mut self, node: Node) {
        self.scoped(|w| {
            if let Some(params) = field(node, "parameters") {
                w.bind_params(params, Origin::Param);
            }
            w.children(node);
        });
    }

    fn local(&mut self, node: Node) {
        let Some(decl) = named(node)
            .into_iter()
            .find(|c| c.kind() == "variable_declaration")
        else {
            return self.children(node);
        };
        self.children(decl);
        self.bind_declaration(decl, Origin::Local);
    }

    fn foreach(&mut self, node: Node) {
        self.scoped(|w| {
            if let Some(left) = field(node, "left") {
                let var = Var {
                    origin: Origin::Local,
                    ty: VarTy::Untyped,
                };
                w.bind(w.text(left), var);
            }
            w.children(node);
        });
    }

    /// Visits a lambda. `typed` gives the catalogue type of each parameter
    /// and the site that types them; an empty type string leaves that
    /// parameter untyped.
    pub(super) fn lambda(&mut self, node: Node, typed: Option<(Vec<&'static str>, usize)>) {
        self.scoped(|w| {
            let names: Vec<(String, Option<String>)> = match field(node, "parameters") {
                Some(p) if p.kind() == "parameter_list" => named(p)
                    .into_iter()
                    .filter(|c| c.kind() == "parameter")
                    .map(|c| {
                        let name = field(c, "name").map(|n| w.text(n)).unwrap_or_default();
                        (name, field(c, "type").map(|t| w.compact(t)))
                    })
                    .collect(),
                Some(p) => vec![(w.text(p), None)],
                None => Vec::new(),
            };
            for (i, (name, written)) in names.into_iter().enumerate() {
                let ty = match (&typed, written) {
                    (Some((types, site)), _) if types.get(i).is_some_and(|t| !t.is_empty()) => {
                        VarTy::Cat(types[i], *site)
                    }
                    (_, Some(written)) => VarTy::Written(written),
                    _ => VarTy::Untyped,
                };
                w.bind(
                    name,
                    Var {
                        origin: Origin::Param,
                        ty,
                    },
                );
            }
            if let Some(body) = field(node, "body") {
                w.visit(body);
            }
        });
    }

    // A class whose base list names a definition type gets a site the
    // endpoint names inside it hang from.
    fn definition_site(&mut self, node: Node, name: &str) -> Option<(usize, &'static str)> {
        let bases = named(node).into_iter().find(|c| c.kind() == "base_list")?;
        for base in named(bases) {
            let written = self.compact(base);
            let Some(entry) = catalogue_type(&written) else {
                continue;
            };
            let Some((_, role)) = DEFINITIONS.iter().find(|(n, _)| *n == entry) else {
                continue;
            };
            let mut types = vec![site_type(Ty::Definition, name)];
            if let Some(arg) = type_args(&written).first() {
                types.push(site_type(*role, arg));
            }
            let index = self.push(TransportSite {
                written: Some(written),
                types,
                ..self.site(line(base), entry.to_string())
            });
            return Some((index, entry));
        }
        None
    }

    fn attribute_sites(&mut self, node: Node, name: &str) {
        for list in named(node)
            .into_iter()
            .filter(|c| c.kind() == "attribute_list")
        {
            for attribute in named(list) {
                self.attribute_site(attribute, name);
            }
        }
    }

    fn attribute_site(&mut self, attribute: Node, decorated: &str) {
        let Some(name) = field(attribute, "name").map(|n| self.compact(n)) else {
            return;
        };
        let written = if name.ends_with("Attribute") {
            name
        } else {
            format!("{name}Attribute")
        };
        let Some(entry) = catalogue_type(&written) else {
            return;
        };
        let Some((_, val, role)) = ATTRIBUTES.iter().find(|(n, _, _)| *n == entry) else {
            return;
        };
        let argument = named(attribute)
            .into_iter()
            .find(|c| c.kind() == "attribute_argument_list")
            .and_then(|list| named(list).into_iter().next())
            .and_then(|arg| named(arg).into_iter().last());
        let value = argument.map_or(
            RawValue::Unknown {
                reason: "expression".to_string(),
            },
            |a| self.read_value(a, *val),
        );
        self.push(TransportSite {
            written: Some(written),
            types: vec![site_type(*role, decorated)],
            values: vec![SiteValue {
                role: val.word().to_string(),
                wrapper: None,
                value,
            }],
            ..self.site(line(attribute), entry.to_string())
        });
    }

    /// A site at `line` for catalogue key `member`, in the current
    /// namespace and type nesting.
    pub(super) fn site(&self, line: usize, member: String) -> TransportSite {
        TransportSite {
            line,
            member,
            parent: None,
            written: None,
            receiver: None,
            namespace: self.ns.clone(),
            outer_types: self.outer.clone(),
            types: Vec::new(),
            values: Vec::new(),
            formatter: None,
            filtered: false,
        }
    }

    pub(super) fn push(&mut self, site: TransportSite) -> usize {
        self.sites.push(site);
        self.sites.len() - 1
    }

    /// The receiver a call or assignment with no receiver expression has.
    pub(super) fn implicit_this(&self) -> Recv {
        match self.def {
            Some((index, ty)) => Recv::Typed {
                ty,
                parent: Some(index),
                written: None,
            },
            None => Recv::Bare,
        }
    }

    /// Resolves a receiver expression, visiting it once.
    pub(super) fn expr(&mut self, node: Node) -> Recv {
        match node.kind() {
            "identifier" => self.lookup_receiver(&self.text(node)),
            "member_access_expression" => self.member_access(node),
            "invocation_expression" => match self.invocation(node) {
                Some(index) => self.returned(index),
                None => Recv::Untyped,
            },
            "this" | "base" => self.implicit_this(),
            "parenthesized_expression" => match named(node).first() {
                Some(inner) => self.expr(*inner),
                None => Recv::Untyped,
            },
            "object_creation_expression" => {
                let ty = field(node, "type").map(|t| self.compact(t));
                self.children(node);
                ty.map_or(Recv::Untyped, Recv::Known)
            }
            _ => {
                self.visit(node);
                Recv::Untyped
            }
        }
    }

    fn returned(&self, index: usize) -> Recv {
        let returns =
            catalogue::member_by_key(&self.sites[index].member).and_then(|(m, _)| m.returns);
        match returns {
            Some(ty) => Recv::Typed {
                ty,
                parent: Some(index),
                written: None,
            },
            None => Recv::Untyped,
        }
    }

    fn lookup_receiver(&self, name: &str) -> Recv {
        self.var_receiver(name, self.lookup(name).map(|v| v.ty.clone()))
    }

    // `this.name` names a member of the enclosing type even where a
    // parameter or local of the same name hides it.
    fn this_member(&self, name: &str) -> Option<Recv> {
        let var = self
            .frames
            .iter()
            .rev()
            .flat_map(|f| f.iter().rev())
            .find(|(n, v)| n == name && v.origin == Origin::Field)
            .map(|(_, v)| v.ty.clone())?;
        Some(self.var_receiver(name, Some(var)))
    }

    fn var_receiver(&self, name: &str, ty: Option<VarTy>) -> Recv {
        match ty {
            Some(VarTy::Cat(ty, site)) => Recv::Typed {
                ty,
                parent: Some(site),
                written: None,
            },
            Some(VarTy::Written(written)) => written_receiver(written),
            Some(VarTy::Untyped) => Recv::Untyped,
            None => match catalogue_type(name) {
                Some(ty) => Recv::Typed {
                    ty,
                    parent: None,
                    written: Some(name.to_string()),
                },
                None => Recv::Untyped,
            },
        }
    }

    fn member_access(&mut self, node: Node) -> Recv {
        let path = self.compact(node);
        if path.contains('.') && super::syntax::is_path(&path) {
            if let Some(ty) = catalogue_type(&path) {
                return Recv::Typed {
                    ty,
                    parent: None,
                    written: Some(path),
                };
            }
        }
        let (Some(inner), Some(name)) = (field(node, "expression"), field(node, "name")) else {
            self.children(node);
            return Recv::Untyped;
        };
        let name = self.text(name);
        if inner.kind() == "this" {
            if let Some(member) = self.this_member(&name) {
                return member;
            }
        }
        let receiver = self.expr(inner);
        self.property_site(node, &name, receiver)
    }

    fn property_site(&mut self, node: Node, name: &str, receiver: Recv) -> Recv {
        let Recv::Typed { ty, .. } = &receiver else {
            return Recv::Untyped;
        };
        let found = catalogue::members_named(name, Form::Property)
            .find_map(|m| m.overloads.iter().find(|o| catalogue::derives(ty, o.on)));
        let Some(overload) = found else {
            return Recv::Untyped;
        };
        let mut site = self.site(line(node), format!("{}.{name}", overload.decl));
        apply_receiver(&mut site, &receiver);
        let index = self.push(site);
        self.returned(index)
    }
}

fn written_receiver(written: String) -> Recv {
    match catalogue_type(&written) {
        Some(ty) => Recv::Typed {
            ty,
            parent: None,
            written: Some(written),
        },
        None => Recv::Known(written),
    }
}

/// Records how the site was reached: its parent site, or the written name
/// the assembly stage must qualify, or the declared type of a chain start's
/// receiver.
pub(super) fn apply_receiver(site: &mut TransportSite, receiver: &Recv) {
    match receiver {
        Recv::Typed {
            parent, written, ..
        } => {
            site.parent = *parent;
            site.written = written.clone();
        }
        Recv::Known(written) => site.receiver = Some(written.clone()),
        Recv::Untyped | Recv::Bare => {}
    }
}

pub(super) fn site_type(role: Ty, name: &str) -> SiteType {
    SiteType {
        role: role.word().to_string(),
        name: name.to_string(),
    }
}

fn join(ns: &str, name: &str) -> String {
    if ns.is_empty() {
        name.to_string()
    } else {
        format!("{ns}.{name}")
    }
}

/// The top-level type arguments of a written generic name.
pub(super) fn type_args(written: &str) -> Vec<String> {
    let Some(open) = written.find('<') else {
        return Vec::new();
    };
    let inner = written[open + 1..]
        .strip_suffix('>')
        .unwrap_or(&written[open + 1..]);
    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for c in inner.chars() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                args.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

/// The base name and arity of a written type name, with `global::` and a
/// nullable marker removed.
pub(crate) fn written_parts(written: &str) -> (String, usize) {
    let written = written.trim_start_matches("global::").trim_end_matches('?');
    let base = written.split('<').next().unwrap_or(written).to_string();
    (base, type_args(written).len())
}

/// The one catalogue type a written name can name, by its simple name and
/// arity, or by its full name when it is written qualified.
pub(crate) fn catalogue_type(written: &str) -> Option<&'static str> {
    let (base, arity) = written_parts(written);
    if base.contains('.') {
        let full = if arity == 0 {
            base
        } else {
            format!("{base}`{arity}")
        };
        return catalogue::type_entry(&full).map(|t| t.name);
    }
    match catalogue::types_named(&base, arity).as_slice() {
        [one] => Some(one.name),
        _ => None,
    }
}
