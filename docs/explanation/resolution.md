# How names resolve

devscout answers from a graph of definitions and references built from source alone. This page
explains how a reference becomes an edge, which tier the edge carries, and the shapes the resolver
decides differently from a compiler. The user-facing summary of each limit is in
[Limitations](../limitations.md); the terms are defined in [Concepts](../concepts.md#glossary).

## Heuristic tiers

**Heuristic `uses-member` edges carry a `tier`: `ext` or `guess`.** `ext` is C#'s own
extension-method lookup — an exact `(member, this-type)` bucket, matched arity, generic unification,
and namespace visibility including enclosing namespaces, vetoed the moment an in-graph receiver
already declares the member — whose one unverifiable case is a receiver outside the graph that
itself declares the member. `guess` is a name match among the defs that declare the member, bounded
by a uniqueness cap, the call-shape rule (a call is never vouched by a property or field), and the
receiver-assignability rule (an external receiver's guess must be nominally assignable to it).
Neither tier's edges become the premise of a further `impact` hop; `--no-guess` drops the `guess`
tier from `refs`/`read`/`impact`/`tests` while keeping `ext`; compact output marks the two `x`/`h`.
Precise edges carry neither `heuristic` nor `tier`.

### Receiver typing

Receiver typing covers `this.` and `base.` qualifiers, `?.` bindings, a local's `await`ed
initializer, cast- and pattern-designated locals, typed `out` parameters, and a one-hop call-chain
tail. A bare unqualified call, a chain more than one hop deep or through `?.`, a lambda parameter
neither lambda rule below types, a receiver typed only by inference the syntax itself does not show,
a `dynamic` receiver, and a `using static` import stay unrecorded, so those shapes resolve through
the untyped name-only tiers or not at all.

### Lambda parameters

The collection-element rule reads the receiver's shape, not its meaning: a call's first
single-parameter lambda takes the element type of any array or single-type-argument generic receiver
(`Task<T>`, `Lazy<T>`) the way `List<T>` gives it.

A lambda handed straight to an in-graph method or extension method has each of its parameters typed
positionally from that method's delegate parameter (`Action<T, ..>`, `Func<T, ..>`, `Predicate<T>`,
`Expression<>` of those, or a declared `delegate`) when every overload that can take it agrees; a
callee outside the graph, a generic delegate parameter, a callee reached through a chain or through
another untyped lambda parameter, a named argument, and a parameter name that two lambdas in one
member bind to different callees leave the parameter untyped.

## Declaring-type binding

**A precise `uses-member` edge binds the type that declares the member, as far as names, arity and
known argument types can tell.** The declaring type in the receiver's static chain — inherited,
overridden and hidden members, interface members through interface, implementing-class and
base-interface receivers, static members through bare, qualified and generic derived type names — is
what the edge targets. An explicit interface implementation is a member only of its interface, so
`this.M()` in a class that both inherits a public `M` and explicitly implements an interface's `M`
binds the base. A same-count overload whose parameter types reject a written argument's known type
is skipped, and the walk continues to the base, sibling-interface or extension overload that accepts
it; an argument of unknown type leaves every same-count overload available.
Two shapes are decided by information the graph does not carry and bind the wrong side of an
`inherits` edge: an `internal new` member hiding a public base member (only `public` counts as
visible to a receiver other than `this`, so an `internal override` is a silent miss instead); and a
`private new` field shadowing a public base property when read from outside its type. Each is pinned
as a known false positive in `fixtures/csharp-direction`.

## Type-argument counts

**A written type-argument count picks between same-named generic siblings.** A receiver written
`Foo<X>` and a base written `Foo` each bind the declaration with that many type parameters, not
whichever of `Foo` and `Foo<T>` the index met first; a name with no declaration at that count keeps
the count-blind answer. A name whose exact count is declared only outside the site's imports now
answers through the same global-uniqueness step a type reference already uses, so a few such sites
resolve elsewhere or turn external instead of binding the wrong count. Two shapes still read as no
type arguments: a receiver typed from a `foreach` variable or a lambda parameter, whose element type
is recorded without its arguments, and the bare `IFoo` of `class X : IFoo, IFoo<int>`, whose base
list keeps one entry per name and carries the generic one.

## Qualified names

**A fully qualified name never falls back to its bare last segment.** A dotted reference whose
exact-qualified lookup fails matches only a def whose full path (a nested type's `+` read as `.`)
ends with the text as written, and is external otherwise: `RabbitMQ.Client.ExchangeType.Fanout` does
not bind an in-tree `ExchangeType`, `System.Text.Json.JsonSerializer.Serialize(x)` does not bind an
in-tree `JsonSerializer`, and `expr.Member.Name` does not bind a nested type named `Member`.
`Outer.Inner` still reaches `Outer+Inner`, `Box<string>.Slot` and `global::App.Widget` are read as
the def paths they spell, `Derived.Item` reaches an `Item` declared inside a base of `Derived`, and
a `using` alias at the head of a dotted name is rewritten to its target and looked up exactly. A
text that several def paths end with stays ambiguous. The scored `guess` tier is unchanged, so a
foreign dotted qualifier can still carry a tagged heuristic edge.

## Nested-type qualifiers

**A static qualifier walks through nested types before its next segment reads as a member.**
`Outer.Inner.Leaf.Value`, with or without a namespace prefix on `Outer`, binds one nested-type
segment at a time from the shortest head that names a type, and emits a single precise edge to
`Outer+Inner+Leaf` with `member` `Value`. The shorter windows of the same chain no longer emit an
edge that names a nested type as if it were a member of its container, which is where a
namespace-qualified head used to bind `Outer` with member `Inner`, and a nested type whose simple
name repeats across containers now binds the container the qualifier names instead of dropping out
as ambiguous. Both the walk and that suppression stand aside for a qualifier the extractor already
typed as an instance receiver, whose name merely coincides with a type's. A chain with no segment
after the nested type, such as `nameof(Outer.Inner)`, emits no `uses-member` edge at all, and a
generic nested type is walked by its arity-less name.

## DI registrations

**A two-type-argument DI service registration records an `implements` edge.** An invocation whose
method name begins `Add` or `TryAdd` and ends `Singleton`, `Scoped` or `Transient` and carries
exactly two type arguments (`services.AddScoped<IContract, Widget>()`) — the keyed and named
spellings included, since the rule is a prefix/suffix shape rather than a fixed name list — records
a type-level `implements` edge from the implementation to the service type, plus a member-level
`implements`/`overrides` edge per implementing/`override` member matched by name and arity; an
ambiguous arity match (two or more candidates) emits nothing. This is what lets `refs`/`read` on a
service interface list its implementations and `impact` on an implementation reach the interface's
own callers through the same widened interface hop a base-list `inherits` edge already uses.
`--no-dispatch` drops both edge kinds from `refs`/`read`/`impact`/`tests`, the same way `--no-guess`
drops the `guess` tier — neither edge kind is ever itself a guess.

## Project model

**The project model reads only `.csproj` and `Directory.Build.props`.** It hand-scans
`ProjectReference`, `Microsoft.NET.Test.Sdk`, and `IsTestProject` — no MSBuild evaluation, no
conditions, no NuGet resolution, and no `.sln`. A file belongs to the nearest ancestor directory
holding exactly one `.csproj`; a directory holding two or more is left unmapped. When a model
exists, a guess never names a def in a project the reference site's project cannot reach, nor a def
in a test project from a non-test site, and a `global using` scopes to the project that declared it.
Without any `.csproj` files, nothing changes.

## Conditional compilation

**Conditional compilation uses the no-build symbol model.** `#if`/`#elif`/`#else`/`#endif` are
evaluated before parsing with no symbol predefined — not `DEBUG`, not `TRACE`, not a
target-framework symbol — so `#if SYMBOL` is inactive, `#else` and `#if !SYMBOL` are active, and at
most one arm of every group is indexed. `#define`/`#undef` inside the file are honored;
`DefineConstants` from a `.csproj` is not read. A condition combines symbols and the `true`/`false`
literals with `!`, `==`, `!=`, `&&`, `||` and parentheses, in that precedence order. A condition
that does not parse is inactive, an `#elif`/`#else`/`#endif` with no group open is ignored, and an
unclosed `#if` runs to the end of the file. Inactive lines are blanked in place (line numbers and
offsets do not move), `#region`/`#pragma`/`#nullable`/`#line`/`#error`/`#warning` are left to the
parser, and a directive-looking line inside a block comment, a verbatim string, or a raw string
literal is not treated as a directive. The `parse` and `spans` diagnostics show the raw tree, both
arms included.

## Generic delegates

**Generic-delegate `typeParams` divergence is under review.** Type-parameter handling for generic
delegate declarations does not yet agree with the rest of the generic ladder; the affected shapes
are under review rather than pinned.

## Compiler facts and the precise tier

**Compiler-fact enrichment measurably moves the `precise` tier's own precision figure.** Once an
admitted artifact is consumed, most references a confirmed compiler fact covers move from the
`precise`/`ext`/`guess` tiers into the `semantic` tier — including references where the compiler
fact merely agrees with what the syntax ladder already found, not only the ones it corrects. What
remains in `tiers.precise` is therefore a different, smaller population (the references no confirmed
fact reached), and its own precision can read measurably lower than the same corpus's syntax-only
`tiers.precise` figure even though the `semantic` tier and the `precise`+`semantic` union both hold
a high floor — see the shipping-gate note in the [compiler facts
guide](../guides/compiler-facts.md#consuming-an-admitted-artifact).

## TypeScript imports

TypeScript and JavaScript file-scope declarations, including declarations in a top-level IIFE body,
become definitions even when they are not exported. Calls, JSX tags and dispatch references to
those definitions resolve within the same file only. Imports and re-exports cannot bind them. A
bare name shared by an exported definition and a file-local definition resolves to the exported
one; an exact `<file>#<name>` id can name the local definition. Manifest purposes still describe
exports only.

Import specifiers resolve through the `paths` and `baseUrl` of the nearest `tsconfig.json` above the
file (its `extends` chain included; a nested config that declares neither falls back to the root
chain), and through re-export barrels up to eight hops deep, with the barrel the source names kept
as `via` on the edge.

**TypeScript references cover imports, calls, JSX tags and dispatches only.** A type annotation,
an `extends`/`implements` clause, a value passed as an argument, a member reached through a chain or
a local (`store.total()`), and a dynamic `import()` record no edge. A named re-export of a name its
target module only re-exports again binds no definition. A package imported by name resolves only
through the repository's own `tsconfig` `paths`/`baseUrl`, so a monorepo importing its own packages
by package name leaves those imports external. JavaScript answers are read but not measured.

## Rust paths

Rust (`.rs`) is indexed and graphed: every `.rs` file in the mapped scope, in every repository,
gives one definition per item (modules, types, traits, functions and methods, constants, statics,
type aliases, `macro_rules!`), which `find` lists and `read` quotes with its doc comments and
attributes, and `refs`, `impact` and `tests` answer on Rust through path resolution: the module tree
from `mod` declarations and `Cargo.toml` crate roots, `use` scope and `pub use` re-exports, calls
(including those inside macro arguments), type uses, `impl Trait for T`, and `#[test]` functions —
with no method-call typing.

**Rust edges come from path resolution only.** `refs`, `impact` and `tests` answer on Rust through
the module tree and `use` scope, but a method call is never typed by its receiver, so `x.f()` gives
no edge; trait dispatch, generics, macro expansion, `cfg` evaluation (every branch is recorded and
resolved) and build-script `include!` are not followed; a `#[path]` carried by `cfg_attr`, or on a
`mod x;` inside an inline module that itself carries `#[path]`, reaches nothing; a path two imports
both provide stays ambiguous rather than guessed. Ids use the file path
(`src/shelf.rs#Widget::new`), not the crate path. A file with an inline `#[cfg(test)]` module is a
test file for `tests` and `impact`, and a helper file with no `#[test]` function is not. Every `.rs`
file in the mapped scope is indexed in every repository, `src/bin/` excepted with every other `bin`
directory; in a mixed repository a Rust item shares the symbol lookup with C# and TypeScript
definitions, so a bare name they share can become ambiguous or resolve to the Rust item — narrow the
scope with `map <dir>` or the `init` scope. See [the Rust construct
catalogue](../reference/rust-constructs.md#known-gaps).
