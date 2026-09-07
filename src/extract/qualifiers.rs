use std::cell::OnceCell;
use std::collections::HashSet;
use std::rc::Rc;

use tree_sitter::Node;

use super::receivers::{collect_member_facts, collect_type_facts, FactTable};
use super::refs::{member_qualifier_info, type_parameter_names};
use super::text::{declared_name, named_children, text};
use super::types::Fact;

// Declarations that own a body a local can be declared in. A local function
// is deliberately NOT here: its locals belong to the enclosing member's flat
// table (collect_member_facts already walked into it), and giving it its own
// scope would hide the enclosing method's locals from it.
pub(super) const MEMBER_SCOPE_NODES: &[&str] = &[
    "method_declaration",
    "constructor_declaration",
    "destructor_declaration",
    "operator_declaration",
    "conversion_operator_declaration",
    "property_declaration",
    "indexer_declaration",
];

// The enclosing type's field/primary-ctor table plus, inside a member
// declaration, that member's own local/param table layered over it.
pub(super) struct Scope<'a> {
    /// Shared by reference with every member scope opened under the same
    /// type declaration.
    type_facts: Rc<FactTable>,
    /// The enclosing type declaration's own type-parameter names, shared the
    /// same way. A member scope unions its own on top when it builds its
    /// member table (see `collect_member_facts`).
    pub(super) type_params: Rc<HashSet<String>>,
    /// The innermost enclosing type's own simple name (no namespace, no
    /// nested-type "+" chain) -- `None` at the root scope (no enclosing
    /// type at all), `Some` from `for_type` on and threaded unchanged
    /// through every member scope opened under it. This is the `owner` an
    /// untyped lambda parameter's bare `this.M(...)`/`M(...)` slot resolves
    /// to (see `lambda_slot_fact`).
    type_name: Option<String>,
    /// `None` at type-body level: a type body is not a member body.
    node: Option<Node<'a>>,
    /// Built on FIRST USE, not on scope entry: most member declarations in a
    /// real file contain no bare-identifier member access at all, and the
    /// scan is a second traversal of the member's subtree. Laziness is
    /// invisible in the output -- the same table, only built on demand.
    member_facts: OnceCell<Option<FactTable>>,
}

impl<'a> Scope<'a> {
    pub(super) fn root() -> Self {
        Scope {
            type_facts: Rc::new(FactTable::new()),
            type_params: Rc::new(HashSet::new()),
            type_name: None,
            node: None,
            member_facts: OnceCell::new(),
        }
    }

    pub(super) fn for_type(node: Node<'a>, src: &[u8]) -> Self {
        let type_params = type_parameter_names(node, src);
        let name = declared_name(node, src);
        Scope {
            type_facts: Rc::new(collect_type_facts(node, src, &type_params)),
            type_params: Rc::new(type_params),
            type_name: if name.is_empty() { None } else { Some(name) },
            node: None,
            member_facts: OnceCell::new(),
        }
    }

    pub(super) fn for_member(&self, node: Node<'a>) -> Self {
        Scope {
            type_facts: Rc::clone(&self.type_facts),
            type_params: Rc::clone(&self.type_params),
            type_name: self.type_name.clone(),
            node: Some(node),
            member_facts: OnceCell::new(),
        }
    }

    // A member-table entry answers even when its
    // value is `None` -- a local whose type nothing vouches for must NOT
    // fall through to a same-named field of a different type.
    fn receiver_fact_for(&self, name: &str, src: &[u8]) -> Option<Fact> {
        let locals = self.member_facts.get_or_init(|| {
            self.node.map(|n| {
                collect_member_facts(
                    n,
                    src,
                    &self.type_params,
                    &self.type_facts,
                    self.type_name.as_deref(),
                )
            })
        });
        if let Some(table) = locals {
            if let Some(found) = table.get(name) {
                return found.clone();
            }
        }
        self.type_facts.get(name).cloned().flatten()
    }

    // `true` when the enclosing MEMBER's own fact table -- locals,
    // parameters, explicitly-typed lambda parameters, patterns and `out`
    // designations, every shape `collect_member_facts` records -- holds ANY
    // entry for `name`, typed or taken-but-unknown. Deliberately never
    // consults `type_facts` (the enclosing TYPE's own fields/properties/
    // primary-ctor params): this answers "is `name` a local in scope", not
    // "does the enclosing scope have a type fact for `name`" --
    // `receiver_fact_for` already answers the latter. This is `receiver_local`'s
    // one source of truth.
    fn has_local_fact(&self, name: &str, src: &[u8]) -> bool {
        let locals = self.member_facts.get_or_init(|| {
            self.node.map(|n| {
                collect_member_facts(
                    n,
                    src,
                    &self.type_params,
                    &self.type_facts,
                    self.type_name.as_deref(),
                )
            })
        });
        locals
            .as_ref()
            .is_some_and(|table| table.contains_key(name))
    }

    // `true` when SOME declaration in scope claims `name` at all -- a local
    // or parameter (`has_local_fact`) OR a field/primary-ctor parameter of
    // the enclosing TYPE -- typed or not. Mirrors `qualifier_type_name`'s own
    // "claimed at all" test (used for `var x = Q.M()`'s call-owner fallback)
    // over `Scope`'s two tables instead of a `collect_member_facts` pass's
    // raw pair: the chain-tail rule (Unit C) needs exactly this to tell "no
    // declaration anywhere claims this bare name, so treat it as a static
    // type reference" apart from "something claims it but vouches for
    // nothing", which `receiver_fact_for` alone collapses into the same
    // `None`.
    fn name_is_claimed(&self, name: &str, src: &[u8]) -> bool {
        self.has_local_fact(name, src) || self.type_facts.contains_key(name)
    }
}

// The member name half of a member-access-shaped ref, normalizing a
// `generic_name` name node ("Foo.Bar<T>(...)") to its bare identifier --
// shared by the direct `a.B` window and the `?.B` binding, which carry the
// member in the same node shape (`identifier` or `generic_name`).
pub(super) fn member_name_text(node: Option<Node>, src: &[u8]) -> Option<String> {
    node.map(|n| {
        if n.kind() == "generic_name" {
            named_children(n)
                .into_iter()
                .find(|c| c.kind() == "identifier")
                .map(|id| text(id, src))
                .unwrap_or_default()
        } else {
            text(n, src)
        }
    })
}

// The receiver-side fields a member-access-shaped ref derives from its
// qualifier node -- shared by the direct `a.B` window and the `a?.B`
// conditional-access window so the two produce byte-identical fields for an
// otherwise-identical qualifier.
pub(super) struct QualifierResolution {
    pub(super) text: String,
    pub(super) generic: bool,
    pub(super) receiver: Option<Fact>,
    pub(super) property_owner: Option<String>,
    pub(super) receiver_base: bool,
    pub(super) receiver_local: bool,
}

pub(super) fn resolve_member_qualifier(
    qualifier: Option<Node>,
    src: &[u8],
    type_stack: &[String],
    scope: &Scope,
) -> Option<QualifierResolution> {
    let kind = qualifier?.kind();
    let (qt, generic) = member_qualifier_info(qualifier, src, type_stack)?;
    // `this`/`base` are bare anonymous tokens in this grammar (verified
    // against the shipped grammar: neither wraps in a
    // `this_expression`/`base_expression` rule), and the ONLY qualifier
    // shape whose receiver is asked of `type_stack` directly rather than of
    // the enclosing scope's local/field fact table -- a coincidentally
    // same-named local or field must never stand in for the enclosing type
    // itself.
    if kind == "this" || kind == "base" {
        let receiver_args = if scope.type_params.is_empty() {
            None
        } else {
            Some(vec!["*".to_string(); scope.type_params.len()])
        };
        return Some(QualifierResolution {
            receiver: Some(Fact {
                type_name: qt.clone(),
                args: receiver_args,
                call: None,
                awaited: false,
                is_array: false,
                lambda: None,
            }),
            text: qt,
            generic,
            property_owner: None,
            receiver_base: kind == "base",
            receiver_local: false,
        });
    }
    // A receiver fact is asked for ONLY for a bare, non-generic qualifier:
    // a dotted qualifier is a flattened chain window (whose head's fact it
    // must never inherit) or a namespace path, and a type-argument list is
    // syntax no local, parameter, or field can carry.
    let dot_at = if generic { None } else { qt.find('.') };
    let bare = dot_at.is_none() && !generic;
    let receiver = if bare {
        scope.receiver_fact_for(&qt, src)
    } else {
        None
    };
    // `receiver_local`: does the enclosing MEMBER's own fact table hold ANY
    // entry for this bare name at all, typed or taken-but-unknown? Answered
    // independently of `receiver` above -- a `None` receiver alone cannot
    // tell "no fact anywhere for this name" apart from "a same-named local
    // is taken but nothing vouches for its type", and the resolver's
    // bare-identifier field/property fallback (Unit B) needs exactly that
    // distinction to let a local always shadow a same-named field.
    let receiver_local = bare && scope.has_local_fact(&qt, src);
    // The head of a TWO-segment chain, and only when the scope vouches for
    // its type: "a.Settings" asks what `a` is, while "x.y.Settings" and a
    // namespace path ask nothing, because a head this file cannot type is a
    // head no property lookup can start from.
    let property_owner = dot_at
        .filter(|d| !qt[d + 1..].contains('.'))
        .and_then(|d| scope.receiver_fact_for(&qt[..d], src))
        .filter(|fact| fact.call.is_none() && fact.lambda.is_none())
        .map(|fact| fact.type_name);
    Some(QualifierResolution {
        text: qt,
        generic,
        receiver,
        property_owner,
        receiver_base: false,
        receiver_local,
    })
}

// The type NAME a chain-tail's HEAD (`a` in `a.B().C()`, for the `.C`
// window) stands for, as far as the file can vouch -- the exact three-way
// answer `qualifier_type_name` gives `Q` in `var x = Q.M()`, reused here
// over `Scope`'s own tables since this runs from `walk()`, not from a
// `collect_member_facts` pass: an in-file fact when one vouches for the
// name (a call-shaped or lambda-slot-shaped fact refused -- one hop, never
// a chain); the bare name itself when nothing in scope claims it at all
// (the static-qualifier shape, `Repo.Load().Validate()`); `None` when
// something claims the name but vouches for no type.
fn chain_tail_receiver_type(a_text: &str, scope: &Scope, src: &[u8]) -> Option<String> {
    match scope.receiver_fact_for(a_text, src) {
        Some(Fact {
            type_name,
            call: None,
            lambda: None,
            ..
        }) => Some(type_name),
        Some(_) => None,
        None if scope.name_is_claimed(a_text, src) => None,
        None => Some(a_text.to_string()),
    }
}

// The chain-tail shape (Unit C): `.C` in `a.B().C()`, whose OWN qualifier is
// the invocation `a.B()`. Returns `(owner, member)` -- the SAME two fields
// `var x = Q.M()` produces for its local, feeding the identical one-hop
// `method_returns` resolver path -- for
//   - a qualifier that is an `invocation_expression`,
//   - whose OWN `function` is a `member_access_expression` (`a.B`) --
//     anything else (a bare call `B()`, a delegate-shaped `Get()()`) is not
//     this shape, and
//   - whose `a` resolves through `member_qualifier_info` to a BARE,
//     non-generic, non-dotted name with a typed receiver (see
//     `chain_tail_receiver_type`).
// A chain of chains (`a.B().C().D()`'s `.D`, whose own `a` is `a.B().C()`,
// an `invocation_expression` `member_qualifier_info` has no arm for) never
// reaches the receiver-typing step at all: `member_qualifier_info` returns
// `None` for it, same as every other shape this function declines.
//
// The third half of the answer is whether `a` was the `base` keyword. A
// `base.` head types as the ENCLOSING type (that is what `type_stack` gives
// it), but the method the hop has to read a return type from is the BASE's,
// not the enclosing type's -- an enclosing type that hides the inherited
// member with one of its own returns something else entirely. The resolver
// reads the bit off the ref's `receiverBase` and starts the hop at the
// bases; `this.` keeps `false` and keeps hopping through the enclosing type.
pub(super) fn resolve_call_chain_tail(
    qualifier: Node,
    src: &[u8],
    type_stack: &[String],
    scope: &Scope,
) -> Option<(String, String, bool)> {
    if qualifier.kind() != "invocation_expression" {
        return None;
    }
    let function = qualifier.child_by_field_name("function")?;
    if function.kind() != "member_access_expression" {
        return None;
    }
    let head = function.child_by_field_name("expression");
    let head_is_base = head.is_some_and(|h| h.kind() == "base");
    let (a_text, a_generic) = member_qualifier_info(head, src, type_stack)?;
    if a_generic || a_text.contains('.') {
        return None;
    }
    let name_node = function.child_by_field_name("name")?;
    let inner_member = if name_node.kind() == "generic_name" {
        named_children(name_node)
            .into_iter()
            .find(|c| c.kind() == "identifier")
            .map(|id| text(id, src))?
    } else {
        text(name_node, src)
    };
    if inner_member.is_empty() {
        return None;
    }
    let owner = chain_tail_receiver_type(&a_text, scope, src)?;
    Some((owner, inner_member, head_is_base))
}
