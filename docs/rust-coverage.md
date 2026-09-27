# Rust construct catalogue

Every Rust construct the extractor meets, with what `map` records for it. Each recorded item is
a definition with a span, which `find` lists and `read` quotes. On top of the items, `map`
resolves Rust paths through the module tree and `use` scope, with no type inference, and records
the calls, type uses and trait implementations they name as edges of the kinds C# already uses,
so `refs`, `impact` and `tests` answer on Rust code. Method calls are never typed by their
receiver. The catalogue is pinned by [`fixtures/rust-definitions/`](../fixtures/rust-definitions/)
with [`tests/cli_rust_definitions.rs`](../tests/cli_rust_definitions.rs) and by
[`fixtures/rust-edges/`](../fixtures/rust-edges/) with
[`tests/cli_rust_edges.rs`](../tests/cli_rust_edges.rs), with the extractor's and resolver's own
unit tests under `src/extract/tests/rs_*.rs` and `src/rsgraph/edge_tests/`.

## Grammar pin

| Crate | Version |
|---|---|
| `tree-sitter-rust` | 0.24.2 |
| `tree-sitter` | 0.27.0 |

`tree-sitter-rust` depends only on `tree-sitter-language` (^0.1) for its ABI, like the C# and
TypeScript grammars, so all four grammars link against the one pinned runtime. Source is parsed
as UTF-16, like every other grammar in the crate, so error recovery does not depend on the
encoding of the input.

## Which files

Every `.rs` file in the mapped scope is indexed, in every repository. Nothing gates it: there is
no `Cargo.toml` probe, flag or setting. The walk's usual skip list applies, so a `.rs` file under
a directory named `target`, `bin`, `obj`, `dist`, `coverage`, `node_modules`, `.next`, `.git` or
`.scout` is not indexed; that includes a crate's `src/bin/` directory. To keep Rust out of a
mixed repository, map a narrower scope (`devscout map <dir>`, or the scope given to `init`).

## The item model

- **Walked contexts.** The file root, inline module bodies (`mod x { ... }`), `impl` bodies,
  `trait` bodies and `extern` block bodies. Nothing inside a function body, an initializer or any
  other expression, and nothing inside a token tree (a macro invocation or a `macro_rules!`
  body), is visited.
- **Kinds.** The Rust keyword: `mod`, `struct`, `enum`, `union`, `trait`, `fn`, `const`,
  `static`, `type` and `macro_rules`.
- **Path.** The enclosing inline-module names, then an owner segment, then the item's name,
  joined by `::`. The owner segment is the self type's head inside an `impl` and the trait's name
  inside a `trait`; an `extern` block adds none. The trait of `impl Trait for Type` is not part of
  the path. A raw identifier is recorded without `r#`.
- **Self-type head.** The last identifier of the self type once generic arguments, references,
  raw pointers, `dyn` and path qualifiers are removed: `Widget<T>`, `crate::shop::Widget`,
  `&'a mut Widget` and `*const Widget` all head as `Widget`, `dyn Render` as `Render`,
  `<T as Tr>::Assoc` as `Assoc`, and a primitive keeps its name (`u32`). Any type with no single
  name (a tuple, array, slice, function pointer, unit, never, or a `+`-bounded trait object)
  heads as `_`.
- **Repeats.** A path that repeats an earlier item's path in the same file gets `-1`, `-2`, ...
  in source order: `fmt` in `impl Display for Gadget` and then in `impl Debug for Gadget` are
  `Gadget::fmt` and `Gadget::fmt-1`; two `cfg`-gated functions sharing a name are `home_shelf`
  and `home_shelf-1`. No path carries whitespace or any of `<>[](){}'"$*?!;&|`, so every id is
  safe as one shell argument.
- **Id and namespace.** The id is `<file>#<path>` with the repository-relative file. The
  namespace is `<file>` for a file-level item, otherwise `<file>#<parent path>`.
- **Lines.** `line` is the first line of the outer doc comments (`///`, `/** */`) and outer
  attributes (`#[...]`) directly above the item; a plain comment among them is passed over but
  never starts the span, and an inner attribute, an inner doc comment (`//!`, `/*! */`), any other
  item or the start of the body ends the backward walk. With none of them, `line` is the item's
  own first line. The name row sits on the name's line, and the span ends on the item's last
  line.
- **Purpose.** A `.rs` manifest entry reads `<prefix> — <segments>`, or either part alone, cut
  at the shared 200-unit cap. The prefix comes from the file's first node after a shebang and
  inner attributes: an inner doc comment gives its first non-empty line (a run of `//!` lines is
  one doc), a plain comment gives its first line (with its markers removed), anything else gives
  none. The segments are `<kind> <name>` for each item declared directly at the file root, grouped
  struct, enum, union, trait, type, fn, const, static, macro_rules, mod, in source order within a
  group, joined by ` | `. A file with neither is recorded as `ast-none` with the heuristic text.

## Reading an item

`read` names a Rust item by its unique name, by its full id (`src/shelf.rs#Gadget::fmt-1`), or
by a `::` path: `read Hinge::oil` and `read cabinet::Hinge::oil` both answer when exactly one
item's path equals the query or ends with it at a `::` boundary, with or without its `-N`
suffix. `read Gadget::fmt` lists both `fmt` items as ambiguous; the suffixed id reads the second.
The `::` step runs after every other resolution step and no C# or TypeScript id contains `::`,
so it never changes a C# or TypeScript answer.

## Constructs

| Construct | Verdict |
|---|---|
| `mod x;` and `mod x { ... }` | recorded, `mod`; an inline body is walked and adds `x` to the path |
| `struct`, `enum`, `union` | recorded with their own kind |
| `trait` | recorded, `trait`; its body is walked and adds the trait's name to the path |
| trait required and provided `fn` | recorded, `fn`, path `Trait::name` |
| associated `type` and `const` in a trait or `impl` | recorded, `type` / `const`, with the owner segment |
| free `fn`, `const`, `static`, `type` alias | recorded with their own kind |
| `impl` methods and associated functions | recorded, `fn`, path `Type::name` (the self type's head) |
| `macro_rules!` | recorded, `macro_rules` |
| `extern` block `fn`, `static`, `type` | recorded with their own kind, no extra segment |
| `impl` block | not recorded: it has no name; its items carry the self type as owner |
| `extern` block itself | not recorded: it has no name |
| `use` declaration, `extern crate` | not recorded: they name other items, they declare none |
| enum variant, struct or union field | not recorded: not an item of its own |
| item inside a function body | not recorded: it has no path a reader could name it by |
| item inside a `const` or `static` initializer, or any other expression | not recorded, for the same reason |
| item inside a macro invocation or a `macro_rules!` body | not recorded: a token tree is not parsed as items |
| item generated by a macro or a derive | not recorded: macros are not expanded |
| item named `_` (`const _: () = ...;`) or by a macro metavariable | not recorded: no name to find it by |
| item inside a region the grammar recovered as `ERROR` | not recorded; items outside the region are |

## Crates and the module tree

- **Crate roots.** Every `Cargo.toml` in the mapped scope with a `[package]` section names its
  roots: the `path` of each `[lib]`, `[[bin]]`, `[[test]]`, `[[example]]` and `[[bench]]` entry,
  plus cargo's default target files that the walk listed (`src/lib.rs`, `src/main.rs`,
  `tests/*.rs`, `examples/*.rs`, `benches/*.rs`, `build.rs`). A `path` that leaves the package
  directory or names no walked file adds nothing. A `[workspace]`-only manifest names no root. The
  manifest is read by a line scan of section headers and `name` and `path` keys.
- **Library name.** `[lib] name`, else `[package] name` with `-` read as `_`, present only when a
  library root exists. A binary, integration test, example or bench root reaches the library by
  that name (`rust_edges::light`, `::rust_edges::light`); the library itself, the build script and
  an orphan root do not.
- **No manifest.** With no `Cargo.toml` in scope, every file named `lib.rs` or `main.rs` is a
  crate root.
- **Orphans.** Every `.rs` file no root reaches is a root of its own, with or without a manifest,
  in path order, and reaches further unreached files the same way.
- **`mod x;`** resolves to `x.rs` or `x/mod.rs` beside a crate root or a `mod.rs` file, and to
  `f/x.rs` or `f/x/mod.rs` from any other file `f.rs`. Inside an inline `mod m { }` it resolves
  one directory deeper per enclosing module. A declaration whose file is missing, or whose two
  candidate files both exist, reaches nothing.
- **First position wins.** Roots are walked in package-directory order, then library, binaries,
  tests, examples, benches and the build script, each breadth first; the first position to reach
  a file places it, and a later declaration still resolves paths into that file.
- **Ids stay file-local.** A module path maps to a file, and a file plus an item path to the id
  `<file>#<path>`; no crate-path id is introduced.
- **Rebuild on a manifest edit.** The package model is rediscovered on every `map` and compared
  with `rust-crates.json` beside the graph. A difference rebuilds the graph, because a manifest is
  not a source file and no walked mtime moves when it changes. A tree with no `Cargo.toml` writes
  no sidecar.

## Binding

- **Namespaces.** The type namespace holds modules, structs, enums, unions, traits and type
  aliases; the value namespace holds functions, consts, statics and tuple and unit structs. The
  two are kept apart.
- **Scope order**, nearest first: the `use` declarations of each enclosing block (its explicit
  imports, then its globs); the module's own items and explicit imports; its globs, which bring in
  every name the target module declares or imports, its own globs included; and last the
  package's library name. std and external crates bind nothing.
- **Globs that cannot be listed.** A glob of std, an external crate or an enum, or one that
  reaches a crate module whose explicit import of the name resolved outside the crate, may provide
  any name. A lookup that finds the name nowhere at such a level stops there with no edge: a call
  inside `{ use std::cmp::*; max(1, 2) }` never binds a crate `max` outside the block, and a
  module-level `use other_crate::*;` shadows the library name in expression paths. A `use` path
  goes on past it: rustc rejects an import whose first segment a glob would shadow from an outer
  scope.
- **Shadowing.** An item declared inside an enclosing block, or a generic parameter, shadows a
  path's first segment. A local binding (a parameter or a `let`, closure, `match`, `if let`,
  `while let` or `for` pattern anywhere in the enclosing function body) shadows a single-segment
  value path. Locals are flow-insensitive: a local that shares a function's name drops a call to
  that function even before the local exists, but a closure call is never bound to an item. Names
  a macro's token trees may bind shadow the same way (see
  [Calls inside macro arguments](#calls-inside-macro-arguments)). An
  explicit import nothing resolves, such as `use std::fmt::Result;`, still shadows a glob of the
  same name.
- **Paths.** `self`, repeated `super`, `crate`, a leading `::` (followed by the library name) and
  the library name name modules directly.
- **`use` trees** are flattened: groups, nested groups, globs, `self` in a group, `as` renames,
  and `as _`, which binds nothing. A `pub use` chain is followed to the defining item; a cycle
  resolves to nothing.
- **Associated items.** `Type::f` finds `f` among the items of every `impl` whose self type
  resolves, in the impl's own scope, to `Type`, and a trait's own items when `Type` is a trait.
  `Self::f` uses the enclosing impl's self type or the enclosing trait. A generic parameter, a
  type alias and a qualified path (`<T as Trait>::f`) bind nothing after them.
- **Ambiguity.** One candidate binds. Two or more give one `ambiguous` edge: `origin` is the kind
  the edge would have had, `raw` is the path as written up to the ambiguous segment, and the
  candidates are sorted by id and capped at five, with the uncapped count beside them. A name
  with no candidate gives no edge; a name is never bound by its bare spelling alone.
- **Visibility is not modelled.** The code compiles, so a path that resolves names an item it can
  reach; a conflict only visibility would settle stays ambiguous.

## Edges

| Construct | Edge |
|---|---|
| call `f(…)`, `m::f(…)`, `Type::f(…)`, `Self::f(…)`, `Trait::f(…)`, with or without a turbofish | `uses-member` to the function's own def, from the line of the segment that names it, `member` its name |
| struct, enum, union, trait or type alias in a type position (parameter, return, field, `let` annotation, generic argument, bound, `where` clause, cast, `impl` self type, trait object, alias right-hand side) | `uses-type` |
| struct literal `S { … }`, tuple-struct constructor `S(…)`, unit struct value `S` | `uses-type` |
| a qualifier segment naming a type: `Type` in `Type::f(…)`, `E` in `E::V` or in a pattern `E::V(x)` | `uses-type` |
| `impl Trait for T` with a crate trait | type-level `implements` from the line of the trait path, and `uses-type` for `T` |
| `impl std::fmt::Display for T` | `uses-type` for `T` only |
| `use` declaration | none: it binds a name |
| method call `x.f(…)`, field access | none: no receiver typing |
| a function, const or static named outside callee position | none |
| a macro invocation's name | none |
| `Self` itself, a module segment | none |

Every Rust edge is precise: no `heuristic`, `tier`, `source` or `overload_signature` field. Rust
edges follow every C# and TypeScript edge, in file order, then source order, then segment order.
No edge kind or field is added and `GRAPH_SCHEMA_VERSION` stays 3.

## Calls inside macro arguments

A macro invocation's arguments are an unparsed token tree, so every invocation's tree, nested
trees and nested invocations included, is scanned for `ident (` and `path :: ident (`, with an
optional turbofish before the parenthesis. A hit binds and gives edges exactly as the same call
would outside a macro, qualifier segments included.

- A name after `.` (a method), `$` (a macro fragment) or `fn` (a definition) is no call, and a
  nested macro's name (`inner!(`) is none either; its own tree is scanned in turn.
- Contextual keywords the grammar tokenizes on their own inside a token tree (`default`, `union`,
  `auto`, `raw`) are ordinary path segments, so `Shelf::default()` is read.
- Not scanned: string literals (so nothing inside a format string), `macro_rules!` bodies,
  attribute arguments and attribute-like macro inputs.
- **Names bound inside a tree shadow.** A macro can bind any identifier it is handed, so every
  identifier a tree holds outside a call or path position may be a binding: a closure, `let`,
  `match`, `if let`, `while let` or `for` pattern, a parameter of a function written inside an
  item-like macro, or a bare argument such as `bind!(f)`. A struct field name before its `:`
  (`Fact { identity: identity(1) }`) is not one. A single-segment call in the same invocation
  that shares a lowercase one gives no edge. Items, generic parameters and `use` names
  declared inside the trees, and an uppercase name outside angle brackets and type positions,
  shadow a path's first segment, and an `impl` or trait inside them drops `Self` paths.
- **Statement macros reach the rest of their block.** For an invocation that stands as a
  statement, its possible lowercase bindings shadow single-segment calls anywhere in the enclosing
  function body, and its possible items shadow paths anywhere in its block, as a local binding or
  a block item would.
- Known misses: calls that exist only after expansion; `$` fragments and method calls; type
  references and struct literals inside macro arguments; calls inside `macro_rules!` bodies; a
  call whose name the same invocation, or a statement macro in the same function, also holds
  outside a call position, since it may be a binding.

## Tests

A function with a `#[test]` attribute, or an attribute whose path ends in `::test`
(`#[tokio::test]`), is a test: its own def carries `testMethods: ["<its name>"]`. `tests` and
`impact` then reach Rust tests through the existing rule that a file declaring a test def is a
test file. So a production file with an inline `#[cfg(test)]` module is a test file for `tests`
and counts in `impact`'s `tests=N`, and a `tests` row lists every test function of its file as a
carrier. A `#[cfg(test)]` module or `tests/` file with no `#[test]` function, such as a helper
module, is not a test file.

## Stats

`stats.edges_by_kind` counts the Rust edges under `uses-member`, `uses-type` and `implements`,
and `test_def_count` counts the Rust tests. `ambiguous_count`, `ambiguous_pct`,
`unresolved_external_count` and the heuristic counters keep their C# meaning and exclude Rust.

## Known gaps

- **No receiver typing, trait dispatch, generics or macro expansion.** A method call gives no
  edge, a call through a generic parameter or a trait object binds nothing, and a call a macro
  generates is invisible.
- **No `cfg` evaluation.** Every `cfg` branch is recorded (with `-N` suffixes where paths
  repeat) and resolved, and a path inside a `cfg`-disabled region binds like any other.
- **`#[path]` is not read** (a module declared with one reaches the default file or nothing), and
  **`src/bin/` is skipped** with every other `bin` directory (see [Which files](#which-files)), so
  its binaries are neither indexed nor roots.
- **Cross-package paths.** A path into another package of the same workspace through a
  dependency's crate name binds nothing; only a package's own library name binds.
- **Helper files are not test files**, and pattern paths give only their qualifiers.
- **A scoped map without the manifest.** Only manifests inside the mapped scope count; a scope
  that leaves out the package's `Cargo.toml` falls back to the no-manifest rule.
- **`audit --semantic` without `--units`** in a repository mixing C# and Rust counts the Rust
  `uses-member` edges in its denominator.
- **Ids are file paths, not crate paths.** `src/shelf.rs#Widget::new`, never
  `crate::shelf::Widget::new`.
- **Shared names in a mixed repository.** Rust items join the same symbol lookup as C# and
  TypeScript definitions, so a bare name shared with a C# or TypeScript symbol can become
  ambiguous in `refs` or `read`, or resolve to the Rust item. Narrow the scope to keep them apart.
- **Hub files.** A Rust file with 34 or more distinct referrers is an `impact` hub like any other,
  and expansion stops there.
- **The read hook's offer inside a module or trait.** On a ranged read inside an inline module or
  a trait, the offer names the enclosing item: both spans overlap the range, and the earlier start
  wins the tie.

## Grammar gaps in this repository

Mapping this repository's own source: every `.rs` file parses without an `ERROR` or `MISSING`
node, so no item is lost to error recovery.

Compared with `rust-analyzer symbols` over the same files, every `fn` item `map` records is one
rust-analyzer reports at the same file, name and line. The functions rust-analyzer reports that
`map` does not are all inside function bodies (directly, or inside an item declared in a
function body, such as a visitor `impl`) or inside a `const` initializer: the contexts the item
model leaves out.
