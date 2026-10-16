//! Matching one call or assignment against the catalogue overloads, and
//! reading the values at the positions the chosen overloads mark.

use tree_sitter::Node;

use super::catalogue::{self, Form, Member, Overload, Ty, Val, P};
use super::site_types::{Part, RawValue, SiteFormatter, SiteValue};
use super::sites::{apply_receiver, catalogue_type, site_type, Origin, Recv, Walk};
use super::syntax::{field, line, named, pieces, string_literal, Piece};
use super::values::{classify, is_name, UNREAD};

/// The syntactic form of one argument, which is all overload matching reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArgForm {
    Lambda,
    NewUri,
    NewOther,
    Str,
    Name,
    Invoke,
    TypeOf,
    Literal,
    Other,
}

/// What the matched overloads agree an argument position holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plan {
    Value(Val, bool),
    Lambda(&'static [&'static str]),
    Filter,
    Formatter,
    TypeOf(Ty),
    Section,
    Skip,
    Disagree(Option<Val>),
}

fn accepts(kind: P, form: ArgForm) -> bool {
    use ArgForm as F;
    match kind {
        P::Val(_) | P::Text => matches!(form, F::Str | F::Name | F::Invoke | F::Other),
        P::Uri(_) => matches!(form, F::NewUri | F::Name | F::Invoke),
        P::Lambda(_) | P::Filter => matches!(form, F::Lambda | F::Name),
        P::Formatter => matches!(form, F::NewOther | F::Name | F::Invoke),
        P::Opaque => matches!(
            form,
            F::NewOther | F::Name | F::Invoke | F::Literal | F::Other | F::TypeOf
        ),
        P::TypeOf(_) => matches!(form, F::TypeOf | F::Name),
        P::Section => matches!(form, F::Invoke | F::Name),
        P::Rest => true,
    }
}

fn fits(overload: &Overload, forms: &[ArgForm]) -> bool {
    for (k, param) in overload.params.iter().enumerate() {
        if param.kind == P::Rest {
            return true;
        }
        match forms.get(k) {
            Some(form) if !accepts(param.kind, *form) => return false,
            None if !param.optional => return false,
            _ => {}
        }
    }
    forms.len() <= overload.params.len()
}

fn param_at(overload: &Overload, index: usize) -> Option<P> {
    for (k, param) in overload.params.iter().enumerate() {
        if param.kind == P::Rest || k == index {
            return Some(param.kind);
        }
    }
    None
}

fn plan_of(kind: P) -> Plan {
    match kind {
        P::Val(v) => Plan::Value(v, false),
        P::Uri(v) => Plan::Value(v, true),
        P::Lambda(types) => Plan::Lambda(types),
        P::Filter => Plan::Filter,
        P::Formatter => Plan::Formatter,
        P::TypeOf(t) => Plan::TypeOf(t),
        P::Section => Plan::Section,
        P::Opaque | P::Text | P::Rest => Plan::Skip,
    }
}

fn merged_plan(matches: &[&Overload], index: usize) -> Plan {
    let kinds: Vec<P> = matches.iter().filter_map(|o| param_at(o, index)).collect();
    let Some(first) = kinds.first() else {
        return Plan::Skip;
    };
    if kinds.iter().all(|k| k == first) {
        return plan_of(*first);
    }
    let val = kinds.iter().find_map(|k| match k {
        P::Val(v) | P::Uri(v) => Some(*v),
        P::Section => Some(Val::Key),
        _ => None,
    });
    Plan::Disagree(val)
}

fn unknown(reason: &str) -> RawValue {
    RawValue::Unknown {
        reason: reason.to_string(),
    }
}

// C# binds a named argument by its parameter name, which the catalogue does
// not keep, and every argument after one out of its place is named too. So
// no value from the first named argument on is read by its position.
fn read_unbound(site: &mut super::site_types::TransportSite, plan: &Plan) {
    let role = match plan {
        Plan::Value(Val::Ignore, _) => return,
        Plan::Value(role, _) => *role,
        Plan::Section => Val::Key,
        Plan::Disagree(Some(role)) => *role,
        Plan::Formatter => {
            site.formatter = Some(SiteFormatter {
                written: None,
                args: Vec::new(),
            });
            return;
        }
        Plan::Filter => {
            site.filtered = true;
            return;
        }
        Plan::TypeOf(_) | Plan::Skip | Plan::Lambda(_) | Plan::Disagree(None) => return,
    };
    site.values.push(SiteValue {
        role: role.word().to_string(),
        wrapper: None,
        value: unknown("overload"),
    });
}

fn is_named(expression: Node) -> bool {
    expression
        .parent()
        .is_some_and(|a| a.kind() == "argument" && field(a, "name").is_some())
}

/// The argument expressions of an argument list, in order.
fn arguments<'t>(list: Option<Node<'t>>) -> Vec<Node<'t>> {
    list.map(named)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.kind() == "argument")
        .filter_map(|a| named(a).into_iter().last())
        .collect()
}

impl Walk<'_> {
    fn form(&self, node: Node) -> ArgForm {
        match node.kind() {
            "lambda_expression" | "anonymous_method_expression" => ArgForm::Lambda,
            "object_creation_expression" => {
                let ty = field(node, "type").map(|t| self.compact(t));
                if ty.and_then(|t| catalogue_type(&t)) == Some(catalogue::URI_TYPE) {
                    ArgForm::NewUri
                } else {
                    ArgForm::NewOther
                }
            }
            "implicit_object_creation_expression" => ArgForm::NewOther,
            "string_literal"
            | "verbatim_string_literal"
            | "raw_string_literal"
            | "interpolated_string_expression" => ArgForm::Str,
            "binary_expression" => {
                let operands = [field(node, "left"), field(node, "right")];
                if operands
                    .iter()
                    .flatten()
                    .any(|o| self.form(*o) == ArgForm::Str)
                {
                    ArgForm::Str
                } else {
                    ArgForm::Other
                }
            }
            "identifier" | "member_access_expression" | "generic_name" => ArgForm::Name,
            "invocation_expression" => ArgForm::Invoke,
            "typeof_expression" => ArgForm::TypeOf,
            "integer_literal" | "real_literal" | "boolean_literal" | "character_literal"
            | "null_literal" => ArgForm::Literal,
            _ => ArgForm::Other,
        }
    }

    /// Visits a call, and records it as a site when it matches the
    /// catalogue. Returns the site's index.
    pub(super) fn invocation(&mut self, node: Node) -> Option<usize> {
        let Some(function) = field(node, "function") else {
            self.children(node);
            return None;
        };
        let (receiver_node, name_node) = match function.kind() {
            "member_access_expression" => match field(function, "name") {
                Some(name) => (field(function, "expression"), name),
                None => {
                    self.children(node);
                    return None;
                }
            },
            "identifier" | "generic_name" => (None, function),
            _ => {
                self.children(node);
                return None;
            }
        };
        let (name, type_args) = self.name_and_type_args(name_node);
        if !catalogue::is_member_name(&name) {
            self.children(node);
            return None;
        }
        let receiver = match receiver_node {
            Some(r) => self.expr(r),
            None => self.implicit_this(),
        };
        let args = arguments(field(node, "arguments"));
        let matched = self.matched(node, &name, &type_args, &receiver, &args, Form::Call);
        self.visit_args(&args, matched.as_ref(), &receiver);
        matched.map(|(index, _)| index)
    }

    pub(super) fn assignment(&mut self, node: Node) {
        let (Some(left), Some(right)) = (field(node, "left"), field(node, "right")) else {
            return self.children(node);
        };
        let name = match left.kind() {
            "identifier" => self.text(left),
            "member_access_expression" => field(left, "name")
                .map(|n| self.text(n))
                .unwrap_or_default(),
            _ => return self.children(node),
        };
        if catalogue::members_named(&name, Form::Assign)
            .next()
            .is_none()
        {
            return self.children(node);
        }
        let receiver = match field(left, "expression") {
            Some(inner) if left.kind() == "member_access_expression" => self.expr(inner),
            _ => self.implicit_this(),
        };
        self.matched(node, &name, &[], &receiver, &[right], Form::Assign);
        self.visit(right);
    }

    fn name_and_type_args(&self, node: Node) -> (String, Vec<String>) {
        if node.kind() != "generic_name" {
            return (self.text(node), Vec::new());
        }
        let parts = named(node);
        let name = parts.first().map(|n| self.text(*n)).unwrap_or_default();
        let args = parts
            .iter()
            .find(|c| c.kind() == "type_argument_list")
            .map(|list| named(*list).into_iter().map(|t| self.compact(t)).collect())
            .unwrap_or_default();
        (name, args)
    }

    fn candidates(
        name: &str,
        form: Form,
        receiver: &Recv,
    ) -> Vec<(&'static Member, &'static Overload)> {
        let mut found = Vec::new();
        for member in catalogue::members_named(name, form) {
            for overload in member.overloads {
                let reachable = match receiver {
                    Recv::Typed { ty, .. } => catalogue::derives(ty, overload.on),
                    Recv::Known(_) | Recv::Untyped => member.start,
                    Recv::Bare => false,
                };
                if reachable {
                    found.push((member, overload));
                }
            }
        }
        found
    }

    // Matches the call, records its site and returns the index with the
    // per-argument plans.
    fn matched(
        &mut self,
        node: Node,
        name: &str,
        type_args: &[String],
        receiver: &Recv,
        args: &[Node],
        form: Form,
    ) -> Option<(usize, Vec<Plan>)> {
        let forms: Vec<ArgForm> = args.iter().map(|a| self.form(*a)).collect();
        let fitting: Vec<(&Member, &Overload)> = Self::candidates(name, form, receiver)
            .into_iter()
            .filter(|(_, o)| o.types.len() == type_args.len() && fits(o, &forms))
            .collect();
        let (member, first) = *fitting.first()?;
        let overloads: Vec<&Overload> = fitting
            .iter()
            .filter(|(m, _)| std::ptr::eq(*m, member))
            .map(|(_, o)| *o)
            .collect();
        let plans: Vec<Plan> = (0..args.len())
            .map(|i| merged_plan(&overloads, i))
            .collect();
        let mut site = self.site(line(node), format!("{}.{name}", first.decl));
        apply_receiver(&mut site, receiver);
        for (written, role) in type_args.iter().zip(first.types) {
            if *role != Ty::Unused {
                site.types.push(site_type(*role, written));
            }
        }
        let named_from = args.iter().position(|a| is_named(*a));
        for (i, (arg, plan)) in args.iter().zip(&plans).enumerate() {
            if named_from.is_some_and(|from| i >= from) {
                read_unbound(&mut site, plan);
            } else {
                self.read_plan(&mut site, *arg, *plan);
            }
        }
        Some((self.push(site), plans))
    }

    fn read_plan(&self, site: &mut super::site_types::TransportSite, arg: Node, plan: Plan) {
        let value = |role: Val, wrapper: Option<String>, value: RawValue| SiteValue {
            role: role.word().to_string(),
            wrapper,
            value,
        };
        match plan {
            Plan::Value(Val::Ignore, _) | Plan::Skip | Plan::Lambda(_) => {}
            Plan::Value(role, wrapped) => {
                let (wrapper, raw) = if wrapped {
                    self.read_wrapped(arg, role)
                } else {
                    (None, self.read_value(arg, role))
                };
                site.values.push(value(role, wrapper, raw));
            }
            Plan::Section => site
                .values
                .push(value(Val::Key, None, self.read_section(arg))),
            Plan::TypeOf(role) => {
                if arg.kind() == "typeof_expression" {
                    if let Some(ty) = field(arg, "type") {
                        site.types.push(site_type(role, &self.compact(ty)));
                    }
                }
            }
            Plan::Formatter => site.formatter = Some(self.read_formatter(arg)),
            Plan::Filter => site.filtered = true,
            Plan::Disagree(Some(role)) => site.values.push(value(role, None, unknown("overload"))),
            Plan::Disagree(None) => {}
        }
    }

    fn visit_args(&mut self, args: &[Node], matched: Option<&(usize, Vec<Plan>)>, receiver: &Recv) {
        for (i, arg) in args.iter().enumerate() {
            let plan = matched.and_then(|(index, plans)| plans.get(i).map(|p| (*index, *p)));
            match plan {
                Some((index, Plan::Lambda(types))) if self.form(*arg) == ArgForm::Lambda => {
                    let resolved = types
                        .iter()
                        .map(|t| match (*t, receiver) {
                            ("$endpoint", Recv::Typed { ty, .. }) => catalogue::endpoint_of(ty),
                            ("$endpoint", _) => catalogue::DEFAULT_ENDPOINT,
                            (other, _) => other,
                        })
                        .collect();
                    self.lambda(*arg, Some((resolved, index)));
                }
                _ => self.visit(*arg),
            }
        }
    }

    /// Reads a value of kind `val` from an expression.
    pub(super) fn read_value(&self, node: Node, val: Val) -> RawValue {
        let all = match pieces(node, self.src) {
            Ok(all) => all,
            Err(reason) => return unknown(reason),
        };
        for piece in &all {
            if let Piece::Ref(path) = piece {
                if let Some(reason) = self.reference_reason(path) {
                    return unknown(reason);
                }
            }
        }
        if all.iter().all(|p| matches!(p, Piece::Lit(..))) {
            let joined: String = all
                .iter()
                .map(|p| match p {
                    Piece::Lit(t, _) | Piece::Ref(t) => t.as_str(),
                })
                .collect();
            return match classify(&joined, val) {
                Ok(parsed) => RawValue::Literal(parsed),
                Err(reason) => unknown(reason),
            };
        }
        if all
            .iter()
            .any(|p| matches!(p, Piece::Lit(t, _) if !is_name(t)))
        {
            return unknown(UNREAD);
        }
        let pieces = all
            .into_iter()
            .map(|p| match p {
                Piece::Lit(_, at) => Part::Lit(at),
                Piece::Ref(path) => Part::Ref(path),
            })
            .collect();
        RawValue::Pieces { pieces }
    }

    // A reference through a parameter, a local or an instance field is
    // never a constant, and `this.` reaches only instance members.
    fn reference_reason(&self, path: &str) -> Option<&'static str> {
        let first = path.split('.').next().unwrap_or(path);
        if first == "this" {
            return Some("field");
        }
        match self.lookup(first).map(|v| v.origin) {
            Some(Origin::Param) => Some("parameter"),
            Some(Origin::Local) => Some("local"),
            Some(Origin::Field) if path.contains('.') => Some("field"),
            _ => None,
        }
    }

    fn read_wrapped(&self, arg: Node, role: Val) -> (Option<String>, RawValue) {
        if self.form(arg) != ArgForm::NewUri {
            return (None, self.read_value(arg, role));
        }
        let wrapper = field(arg, "type").map(|t| self.compact(t));
        let inner = arguments(field(arg, "arguments"));
        match inner.as_slice() {
            [one] => (wrapper, self.read_value(*one, role)),
            _ => (wrapper, unknown("expression")),
        }
    }

    // `X.GetSection("key")`, the section a `Configure<T>` call binds.
    fn read_section(&self, arg: Node) -> RawValue {
        if arg.kind() != "invocation_expression" {
            return self.read_value(arg, Val::Key);
        }
        let callee = field(arg, "function")
            .filter(|f| f.kind() == "member_access_expression")
            .and_then(|f| field(f, "name"))
            .map(|n| self.text(n));
        let args = arguments(field(arg, "arguments"));
        match (callee.as_deref(), args.as_slice()) {
            (Some("GetSection"), [key]) => self.read_value(*key, Val::Key),
            _ => unknown("method-call"),
        }
    }

    fn read_formatter(&self, arg: Node) -> SiteFormatter {
        match arg.kind() {
            "member_access_expression" => {
                let instance = field(arg, "name").map(|n| self.text(n));
                let written = field(arg, "expression").map(|e| self.compact(e));
                SiteFormatter {
                    written: written.filter(|_| instance.as_deref() == Some("Instance")),
                    args: Vec::new(),
                }
            }
            "object_creation_expression" => {
                let args = arguments(field(arg, "arguments"));
                let named_from = args.iter().position(|a| is_named(*a)).unwrap_or(args.len());
                SiteFormatter {
                    written: field(arg, "type").map(|t| self.compact(t)),
                    args: args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            if i < named_from {
                                self.formatter_argument(*a)
                            } else {
                                unknown("overload")
                            }
                        })
                        .collect(),
                }
            }
            _ => SiteFormatter {
                written: None,
                args: Vec::new(),
            },
        }
    }

    fn formatter_argument(&self, arg: Node) -> RawValue {
        let literal = matches!(
            arg.kind(),
            "boolean_literal" | "character_literal" | "integer_literal"
        );
        if literal || string_literal(arg, self.src).is_some() {
            let text = match string_literal(arg, self.src) {
                Some(Ok(t)) => t,
                Some(Err(reason)) => return unknown(reason),
                None => self.text(arg).trim_matches('\'').to_string(),
            };
            return match classify(&text, Val::FormatterArg) {
                Ok(parsed) => RawValue::Literal(parsed),
                Err(reason) => unknown(reason),
            };
        }
        self.read_value(arg, Val::FormatterArg)
    }
}
