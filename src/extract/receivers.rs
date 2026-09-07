use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::lambdas::{
    lambda_argument_slot, lambda_first_arg_receiver, lambda_receiver_element_fact,
    lambda_slot_fact, DeferredLambdaParam,
};
use super::refs::{
    base_type_identifier, generic_arg_descriptors, member_qualifier_info, type_parameter_names,
};
use super::text::{declared_name, named_children, text};
use super::types::Fact;

// ---------------------------------------------------------------------------
// Receiver facts.
//
// Two flat name->type tables per member ref: one for the enclosing TYPE
// (fields + primary-constructor parameters, which are in scope for the whole
// body) and one for the enclosing MEMBER declaration (its parameters and
// every local declared anywhere inside it). The member table shadows the type
// table: a local or parameter (innermost) wins over a field.
//
// The member table is deliberately FLAT -- a permitted simplification -- so
// two sibling blocks each declaring `x` with a different type
// collapse to one CONFLICTED entry rather than to a per-block answer. A
// conflicted entry, and an entry whose type yields no fact at all
// (`var x = SomeCall()`, `string s`), are both stored as `None`: the name is
// taken, and nothing vouches for its type, so no fact is produced. Storing
// them rather than omitting them is what keeps a local from silently falling
// through to a same-named field of a different type.
// ---------------------------------------------------------------------------

// name -> `Some(fact)` when exactly one fact vouches for it, `None` when the
// name is taken but nothing does. Both the table and its "taken but unknown"
// entries live in one map; the `Option<Fact>` value is that empty slot.
pub(super) type FactTable = HashMap<String, Option<Fact>>;

fn add_fact(table: &mut FactTable, name: Option<String>, fact: Option<Fact>) {
    let Some(name) = name.filter(|n| !n.is_empty()) else {
        return;
    };
    match table.get(&name) {
        None => {
            table.insert(name, fact);
        }
        Some(existing) => {
            if *existing != fact {
                table.insert(name, None);
            }
        }
    }
}

// The declared-type half stays `keep_predefined = false` --
// a `string s` local still vouches for nothing.
pub(super) fn type_fact(
    type_node: Option<Node>,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Option<Fact> {
    let type_name = base_type_identifier(type_node, src, false)?;
    let args = generic_arg_descriptors(type_node, src, type_params);
    // The array bit is read off the type node's OWN top-level kind, before
    // any unwrapping -- `base_type_identifier`/`generic_arg_descriptors`
    // both already look THROUGH an `array_type` to its element, so this is
    // the only place left that still knows the wrapper was there at all.
    let is_array = type_node.is_some_and(|n| n.kind() == "array_type");
    Some(Fact {
        type_name,
        args,
        call: None,
        awaited: false,
        is_array,
        lambda: None,
    })
}

// A direct named child of `node` with kind `target`, or -- when the direct
// child is an `await_expression` -- that same kind one level INSIDE it:
// `var x = await Q.M()` puts `await_expression` as the declarator's direct
// child, with `invocation_expression`/`object_creation_expression` one
// level further in (confirmed against the shipped grammar's own
// `await_expression` -- a single unnamed `expression` child). Exactly one
// level: an `await` wrapping another `await` does not unwrap twice. The
// returned `bool` is `true` only when the match came from inside the
// `await_expression` layer -- `invocation_call` reads it to record whether
// the callee it found was awaited; `new_expression_fact` discards it, since
// an object-creation fact never carries a call and awaited-ness has nothing
// to unwrap there.
fn find_child_through_await<'a>(node: Node<'a>, target: &str) -> Option<(Node<'a>, bool)> {
    for c in named_children(node) {
        if c.kind() == target {
            return Some((c, false));
        }
        if c.kind() == "await_expression" {
            if let Some(inner) = named_children(c).into_iter().find(|g| g.kind() == target) {
                return Some((inner, true));
            }
        }
    }
    None
}

// `var x = new T(...)` -- the ONLY shape where an initializer is consulted.
// An explicitly typed declaration is answered by its own type node, so
// `object o = new Widget()` records `object` (a predefined type: no fact),
// never `Widget`. `var x = await new T()` -- syntactically legal even
// though never awaitable -- is looked through the same one level as
// `invocation_call` looks through it.
fn new_expression_fact(
    declarator: Node,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Option<Fact> {
    let (init, _awaited) = find_child_through_await(declarator, "object_creation_expression")?;
    type_fact(init.child_by_field_name("type"), src, type_params)
}

// `var x = (T)e` -- same declarator scan as `new_expression_fact`, for a
// cast's own `type` field. Not look-through-await: the design that asked
// for the await unwrap (Unit A1 point 2) named only `invocation_call` and
// `new_expression_fact`, and a declarator can carry at most one of
// {object_creation_expression, invocation_expression, cast_expression} as
// its direct initializer, so the three helpers never compete for the same
// child.
fn cast_expression_fact(
    declarator: Node,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Option<Fact> {
    let cast = named_children(declarator)
        .into_iter()
        .find(|c| c.kind() == "cast_expression")?;
    type_fact(cast.child_by_field_name("type"), src, type_params)
}

// Fields and primary-constructor parameters of one type declaration. Direct
// children only: a nested type gets its OWN table, never the enclosing
// type's, because a nested type cannot reach an outer instance field.
pub(super) fn collect_type_facts(
    node: Node,
    src: &[u8],
    type_params: &HashSet<String>,
) -> FactTable {
    let mut table = FactTable::new();
    for kid in named_children(node) {
        if kid.kind() != "parameter_list" {
            continue;
        }
        for p in named_children(kid) {
            if p.kind() != "parameter" {
                continue;
            }
            add_fact(
                &mut table,
                p.child_by_field_name("name").map(|n| text(n, src)),
                type_fact(p.child_by_field_name("type"), src, type_params),
            );
        }
    }
    let Some(body) = node.child_by_field_name("body") else {
        return table;
    };
    collect_declared_member_facts(&named_children(body), src, type_params, &mut table);
    table
}

// The declared-member half of `collect_type_facts`, over a plain member slice
// -- shared with the ERROR-recovery path, where the same declarations exist as
// flat siblings because the parser never built the type node that would have
// wrapped them. A property's declared type vouches for its own name
// exactly like a field's does: one table, one conflict rule, and an indexer is
// a different grammar node so it is excluded by construction.
fn collect_declared_member_facts(
    members: &[Node],
    src: &[u8],
    type_params: &HashSet<String>,
    table: &mut FactTable,
) {
    for c in members {
        if c.kind() == "property_declaration" {
            let name = declared_name(*c, src);
            add_fact(
                table,
                Some(name),
                type_fact(c.child_by_field_name("type"), src, type_params),
            );
            continue;
        }
        if c.kind() != "field_declaration" {
            continue;
        }
        let Some(vd) = named_children(*c)
            .into_iter()
            .find(|k| k.kind() == "variable_declaration")
        else {
            continue;
        };
        let fact = type_fact(vd.child_by_field_name("type"), src, type_params);
        for decl in named_children(vd) {
            if decl.kind() != "variable_declarator" {
                continue;
            }
            add_fact(
                table,
                decl.child_by_field_name("name").map(|n| text(n, src)),
                fact.clone(),
            );
        }
    }
}

// Every `parameter` (explicitly typed lambda parameters included -- an
// implicit_parameter has no type node and therefore no fact) and every
// `variable_declaration` in the member's whole subtree, including the ones
// inside nested lambdas and local functions. One pass, one table.
//
// `class_type_params` is the enclosing type's parameter set; this member's own
// are unioned onto it, which is what turns
// `EventPipelineBinder<FutureState, TM>` into ["FutureState", "*"] when TM is
// the method's own parameter. A local function's own type parameters are
// deliberately NOT unioned in -- its locals land in this same flat table, and
// a local-function parameter used as a type argument records its literal name
// instead, which simply fails to unify. Narrower than the language, never
// wider.
pub(super) fn collect_member_facts(
    node: Node,
    src: &[u8],
    class_type_params: &HashSet<String>,
    type_facts: &FactTable,
    enclosing_type: Option<&str>,
) -> FactTable {
    let mut type_params = class_type_params.clone();
    type_params.extend(type_parameter_names(node, src));
    let mut table = FactTable::new();
    // `var x = Q.M(...)` is settled in a SECOND pass over the
    // collected declarations rather than during the walk: `Q`'s own fact has to
    // be FINAL before it can be read, and the table is flat, so a sibling block
    // declaring `Q` differently cancels it to no fact at all.
    let mut deferred: Vec<DeferredCall> = Vec::new();
    // `foreach (var item in collection)` needs a second pass too:
    // the collection may be a local declared anywhere in this same flat
    // table, including one declared AFTER this foreach in source order.
    let mut deferred_foreach: Vec<DeferredForeach> = Vec::new();
    // A single-parameter lambda's element-type fact (Unit C) needs the same
    // second pass, for the same reason: its collection receiver may itself
    // be a local settled only below (a `Q.M()` callee owner, a foreach
    // variable), or a field this function's OWN first-pass walk never sees
    // at all until `type_facts` is consulted here.
    let mut deferred_lambda: Vec<DeferredLambdaParam> = Vec::new();
    visit_member_facts(
        node,
        src,
        &type_params,
        &mut table,
        &mut deferred,
        &mut deferred_foreach,
        &mut deferred_lambda,
    );
    if !deferred.is_empty() {
        let pending: HashSet<&str> = deferred.iter().map(|d| d.name.as_str()).collect();
        for d in &deferred {
            // A qualifier that is itself one of these locals is refused rather
            // than read from a half-settled table: one hop, never a chain. The
            // refusal still stores the name as TAKEN, which is what keeps it
            // from falling through to a same-named field of a different type.
            let owner = if pending.contains(d.qualifier.as_str()) {
                None
            } else {
                qualifier_type_name(&table, type_facts, &d.qualifier)
            };
            let fact = owner.map(|type_name| Fact {
                type_name,
                args: None,
                call: Some(d.member.clone()),
                awaited: d.awaited,
                is_array: false,
                lambda: None,
            });
            add_fact(&mut table, Some(d.name.clone()), fact);
        }
    }
    for d in &deferred_foreach {
        // Unlike the call-owner qualifier above, no explicit
        // "one hop, never a chain" refusal is needed for the collection: the
        // derived fact never carries a type argument (see
        // `collection_element_fact`), so reading a SIBLING foreach variable
        // that has not settled yet -- or that settled to no fact at all --
        // both read as "no single type argument", the same refusal an
        // ordinary unresolvable collection gets. Order among these entries
        // therefore cannot change the answer.
        let fact = collection_element_fact(&table, type_facts, &d.collection);
        add_fact(&mut table, Some(d.name.clone()), fact);
    }
    // Same "refuse a half-settled sibling" rule as the deferred-call loop
    // above, over the deferred lambda parameters' own names: a slot's
    // qualifier that is itself one of these names is refused rather than
    // read from a half-settled table (one hop, never a chain).
    let pending_lambda: HashSet<&str> = deferred_lambda.iter().map(|d| d.name.as_str()).collect();
    for d in &deferred_lambda {
        // Same reasoning as the foreach loop above -- and the SAME
        // no-explicit-refusal shortcut applies for the same reason:
        // `lambda_receiver_element_fact` only ever unwraps an array or a
        // single-argument generic, never a call fact, so a sibling
        // lambda parameter sharing this one's collection name (however
        // it settles) can only ever read as "no such shape" here, same as
        // an ordinary unresolvable receiver. The collection-element rule
        // (Unit C) is tried FIRST and, when it declines, the callee slot
        // is the fallback -- `orders.Where(o => o.Validate())`
        // types `o` as the element and never records a slot for it.
        let fact = d
            .collection
            .as_deref()
            .and_then(|c| lambda_receiver_element_fact(&table, type_facts, c))
            .or_else(|| {
                d.slot.as_ref().and_then(|s| {
                    lambda_slot_fact(&table, type_facts, enclosing_type, &pending_lambda, s)
                })
            });
        add_fact(&mut table, Some(d.name.clone()), fact);
    }
    table
}

// One `var x = Q.M(...)` local awaiting the second pass: the local's name,
// the two halves of the callee its type depends on, and whether the call was
// awaited (`var x = await Q.M()`) -- carried straight through to the settled
// fact's own `awaited` bit.
struct DeferredCall {
    name: String,
    qualifier: String,
    member: String,
    awaited: bool,
}

// One `foreach (var item in collection)` local awaiting the
// second pass: the loop variable's name, and the bare identifier of the
// collection its element type depends on.
struct DeferredForeach {
    name: String,
    collection: String,
}

// The names one LINQ query clause BINDS -- `from d in xs`, `join o in ys`,
// `join ... into g`, `let n = ...`, and the query continuation `... into g`.
// Every one is a range variable: a name the rest of the query reads and the
// enclosing type never declared. Nothing here vouches for its TYPE (the
// element type of the source sequence is exactly what this extractor cannot
// compute), so the caller records each as taken-but-unknown -- which is the
// whole point: without an entry, the resolver's bare-identifier fallback
// would type `d` from a same-named field of the enclosing type or one of its
// bases and emit a precise edge the language never binds.
fn query_binding_names(n: Node, src: &[u8]) -> Vec<String> {
    match n.kind() {
        // `from [T] d in xs` -- the only clause that names its range
        // variable with a field.
        "from_clause" => n
            .child_by_field_name("name")
            .map(|x| text(x, src))
            .into_iter()
            .collect(),
        // `join [T] o in ys on a equals b`, `let n = ...`, `into g`: the
        // bound name is the FIRST bare identifier child that is not the
        // optional type node (a type can itself be an `identifier`).
        // Everything after it -- a `let`'s value, a join's source and its
        // two key expressions -- is an expression this must not claim.
        "join_clause" | "let_clause" | "join_into_clause" => {
            let type_id = n.child_by_field_name("type").map(|t| t.id());
            named_children(n)
                .into_iter()
                .find(|c| c.kind() == "identifier" && Some(c.id()) != type_id)
                .map(|c| text(c, src))
                .into_iter()
                .collect()
        }
        // The query continuation `... into g` has no node of its own -- the
        // grammar's query-body rule is hidden -- so its identifier sits as a
        // direct child of the `query_expression`, the one place a bare
        // identifier can appear there at all.
        "query_expression" => named_children(n)
            .into_iter()
            .filter(|c| c.kind() == "identifier")
            .map(|c| text(c, src))
            .collect(),
        _ => Vec::new(),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one dispatch over every member-fact shape a node can carry; each shape's deferral rule only makes sense read against the others"
)]
fn visit_member_facts(
    n: Node,
    src: &[u8],
    type_params: &HashSet<String>,
    table: &mut FactTable,
    deferred: &mut Vec<DeferredCall>,
    deferred_foreach: &mut Vec<DeferredForeach>,
    deferred_lambda: &mut Vec<DeferredLambdaParam>,
) {
    if n.kind() == "parameter" {
        let name = n.child_by_field_name("name").map(|x| text(x, src));
        let receiver = lambda_first_arg_receiver(n);
        let slot = lambda_argument_slot(n, src);
        // Deferred to the second pass instead of recorded as
        // taken-but-unknown here whenever EITHER shape matched: recording
        // it now (even as `None`) would permanently conflict with the
        // settled element/slot fact `add_fact` writes later for the SAME
        // name.
        if receiver.is_some() || slot.is_some() {
            if let Some(name) = name.filter(|nm| !nm.is_empty()) {
                deferred_lambda.push(DeferredLambdaParam {
                    name,
                    collection: receiver.map(|r| text(r, src)),
                    slot,
                });
            }
        } else {
            add_fact(
                table,
                name,
                type_fact(n.child_by_field_name("type"), src, type_params),
            );
        }
    } else if n.kind() == "implicit_parameter" {
        // An implicit lambda parameter (`x => ...`) has no `parameter` node
        // at all, so it never reaches the branch above.
        let name = text(n, src);
        let receiver = lambda_first_arg_receiver(n);
        let slot = lambda_argument_slot(n, src);
        // Deferred for the reason the `parameter` branch defers: an entry
        // written now would conflict with the element/slot fact the second
        // pass settles for this very name.
        if receiver.is_some() || slot.is_some() {
            if !name.is_empty() {
                deferred_lambda.push(DeferredLambdaParam {
                    name,
                    collection: receiver.map(|r| text(r, src)),
                    slot,
                });
            }
        } else {
            // Outside a qualifying invocation neither rule types anything
            // -- but the name is still DECLARED here, so it is
            // taken-but-unknown rather than unentered. A lambda parameter
            // that shares its name with a field of the enclosing type
            // shadows that field in C#, and without the entry the
            // resolver's bare-identifier fallback would type it from the
            // field and emit a precise edge to a member the call can
            // never reach.
            add_fact(table, Some(name), None);
        }
    } else if n.kind() == "variable_declaration" {
        let type_node = n.child_by_field_name("type");
        let is_var = type_node.map(|t| t.kind()) == Some("implicit_type");
        let declared = if is_var {
            None
        } else {
            type_fact(type_node, src, type_params)
        };
        for decl in named_children(n) {
            if decl.kind() != "variable_declarator" {
                continue;
            }
            let name = decl.child_by_field_name("name").map(|x| text(x, src));
            let fact = if is_var {
                new_expression_fact(decl, src, type_params)
                    .or_else(|| cast_expression_fact(decl, src, type_params))
            } else {
                declared.clone()
            };
            let call = match (is_var && fact.is_none(), name.as_deref()) {
                (true, Some(n)) if !n.is_empty() => invocation_call(decl, src),
                _ => None,
            };
            match call {
                Some((qualifier, member, awaited)) => deferred.push(DeferredCall {
                    name: name.unwrap_or_default(),
                    qualifier,
                    member,
                    awaited,
                }),
                None => add_fact(table, name, fact),
            }
        }
    } else if n.kind() == "foreach_statement" {
        // foreach_statement carries its own `type`/`left` fields,
        // not a variable_declaration node, so it needs its own rule: an
        // explicitly typed loop variable is answered by that type node
        // exactly like any other declaration; a `var` loop variable is
        // answered by the COLLECTION's own fact, when that fact carries
        // exactly one top-level type argument (settled below, second pass);
        // anything else stays taken-but-unknown. A destructuring
        // `foreach (var (a, b) in ...)` has no single name to record and is
        // left alone entirely -- neither a fact nor a taken slot.
        let left = n.child_by_field_name("left");
        let name = left
            .filter(|l| l.kind() == "identifier")
            .map(|l| text(l, src));
        if let Some(name) = name {
            let type_node = n.child_by_field_name("type");
            let is_var = type_node
                .map(|t| t.kind() == "implicit_type")
                .unwrap_or(true);
            if !is_var {
                add_fact(table, Some(name), type_fact(type_node, src, type_params));
            } else {
                let right = n.child_by_field_name("right");
                match right.filter(|r| r.kind() == "identifier") {
                    Some(r) => deferred_foreach.push(DeferredForeach {
                        name,
                        collection: text(r, src),
                    }),
                    None => add_fact(table, Some(name), None),
                }
            }
        }
    } else if matches!(
        n.kind(),
        "declaration_pattern" | "declaration_expression" | "catch_declaration"
    ) {
        // `if (e is T t)` (also switch statement case patterns and switch
        // expression arms, same `declaration_pattern` node), `out T x`
        // (`declaration_expression`) and `catch (T e)`
        // (`catch_declaration`): each a {type, name} pair, the same shape
        // as `parameter`, just in pattern/argument/handler position. A
        // caught exception is as real a declaration as a local, and it
        // shadows a same-named field of the enclosing type, so it gets the
        // TYPE its handler names rather than merely taking the name. The
        // designation is OPTIONAL for all three -- a discard `_`, a
        // parenthesized deconstruction, a bare `catch (T)` -- and
        // `add_fact` with `None` is already a no-op. `out var x`'s `type`
        // field is `implicit_type`, which `type_fact` already answers with
        // `None` for -- recorded as taken-but-unknown rather than left
        // unentered, which is what lets it shadow a same-named field
        // instead of silently inheriting that field's type.
        add_fact(
            table,
            n.child_by_field_name("name").map(|x| text(x, src)),
            type_fact(n.child_by_field_name("type"), src, type_params),
        );
    } else if matches!(
        n.kind(),
        "from_clause" | "join_clause" | "join_into_clause" | "let_clause" | "query_expression"
    ) {
        // Every name a LINQ query binds is taken-but-unknown: in scope for
        // the rest of the query, never a field of the enclosing type, and
        // with no type this extractor can read off the syntax. See
        // `query_binding_names`.
        for name in query_binding_names(n, src) {
            add_fact(table, Some(name), None);
        }
    }
    for c in named_children(n) {
        visit_member_facts(
            c,
            src,
            type_params,
            table,
            deferred,
            deferred_foreach,
            deferred_lambda,
        );
    }
}

// The (qualifier, member, awaited) triple of a `var x = Q.M(...)`
// initializer, or `None` for every other shape. The qualifier must be BARE and
// non-generic for the same reason a receiver fact's is: a dotted or computed
// qualifier is not a name the ladder can put a type behind. A bare call
// (`var x = M()`) has no qualifier at all and is deliberately not covered.
// `awaited` is `true` only when the invocation sat one level inside an
// `await_expression` (`var x = await Q.M()`) -- straight from
// `find_child_through_await`'s own signal, never re-derived.
fn invocation_call(declarator: Node, src: &[u8]) -> Option<(String, String, bool)> {
    let (init, awaited) = find_child_through_await(declarator, "invocation_expression")?;
    let function = init.child_by_field_name("function")?;
    if function.kind() != "member_access_expression" {
        return None;
    }
    // This scan runs outside the walk()/type_stack traversal, so `this`/
    // `base` qualifiers here have no enclosing type to resolve against and
    // deliberately fall through to no candidate -- the same outcome an
    // empty type_stack would give inside walk(), and unchanged from before
    // `member_qualifier_info` learned those two keywords: `var x =
    // this.M()` never earned a call fact before this branch existed and
    // still does not.
    let (qualifier, generic) =
        member_qualifier_info(function.child_by_field_name("expression"), src, &[])?;
    if generic || qualifier.contains('.') {
        return None;
    }
    let name_node = function.child_by_field_name("name")?;
    let member = if name_node.kind() == "generic_name" {
        named_children(name_node)
            .into_iter()
            .find(|c| c.kind() == "identifier")
            .map(|id| text(id, src))?
    } else {
        text(name_node, src)
    };
    if member.is_empty() {
        return None;
    }
    Some((qualifier, member, awaited))
}

// The type NAME a bare qualifier stands for, as far as the file can vouch: the
// fact's own type when one vouches for the name; `None` when the name is taken
// but nothing vouches for it, when what vouches is itself a call fact (one
// hop, never a chain), or when what vouches is itself a lambda slot fact
// (same one-hop refusal -- its `type_name` is an empty placeholder, never a
// real type); and the text itself when no declaration in scope claims the
// name at all -- an unclaimed bare qualifier is a type name, which is the
// static-call shape.
pub(super) fn qualifier_type_name(
    locals: &FactTable,
    type_facts: &FactTable,
    name: &str,
) -> Option<String> {
    match locals.get(name).or_else(|| type_facts.get(name)) {
        None => Some(name.to_string()),
        Some(Some(fact)) if fact.call.is_none() && fact.lambda.is_none() => {
            Some(fact.type_name.clone())
        }
        Some(_) => None,
    }
}

// The collection's OWN fact, read the same two tables
// `qualifier_type_name` reads, but never falling back to the bare name as a
// type: an unclaimed identifier is nobody's collection, not a static type to
// guess with. A single top-level type argument is the only shape that
// vouches for an element type; a wildcard (the enclosing declaration's own
// type parameter) vouches for nothing here either, because nothing at this
// site knows what it is bound to.
fn collection_element_fact(locals: &FactTable, type_facts: &FactTable, name: &str) -> Option<Fact> {
    match locals.get(name).or_else(|| type_facts.get(name)) {
        Some(Some(fact)) => match fact.args.as_deref() {
            Some([arg]) if arg != "*" => Some(Fact {
                type_name: arg.clone(),
                args: None,
                call: None,
                awaited: false,
                is_array: false,
                lambda: None,
            }),
            _ => None,
        },
        _ => None,
    }
}
