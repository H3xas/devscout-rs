# Rust construct catalogue

Every Rust construct the extractor meets, with what `map` records for it. Rust support is
definitions only: each recorded item is a definition with a span, which `find` lists and `read`
quotes. No reference, import, call or `implements` fact is recorded, so `refs`, `impact` and
`tests` give no Rust rows. The catalogue is pinned by
[`fixtures/rust-definitions/`](../fixtures/rust-definitions/) and
[`tests/cli_rust_definitions.rs`](../tests/cli_rust_definitions.rs), with the extractor's own
unit tests in `src/extract/tests/rs_fragment.rs` and `src/extract/tests/rs_purpose.rs`.

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

## Known gaps

- **No edges.** Module tree, `use` resolution, calls, `implements` and test markers are not
  recorded, so `refs`, `impact` and `tests` give no Rust rows.
- **No `cfg` evaluation and no macro expansion.** Every `cfg` branch is recorded (with `-N`
  suffixes where paths repeat); items a macro generates are invisible.
- **Ids are file paths, not crate paths.** `src/shelf.rs#Widget::new`, never
  `crate::shelf::Widget::new`: a crate path needs the module tree.
- **Shared names in a mixed repository.** Rust items join the same resolution ladder as C# and
  TypeScript definitions, so a bare name shared with a C# or TypeScript symbol can become
  ambiguous, or resolve to the Rust item. Narrow the scope to keep them apart.
- **The read hook's offer inside a module or trait.** On a ranged read inside an inline module or
  a trait, the offer names the enclosing item: both spans overlap the range, and the earlier start
  wins the tie.
- **`src/bin/` is skipped** with every other `bin` directory (see [Which files](#which-files)).

## Grammar gaps in this repository

Mapping this repository's own source: every `.rs` file parses without an `ERROR` or `MISSING`
node, so no item is lost to error recovery.

Compared with `rust-analyzer symbols` over the same files, every `fn` item `map` records is one
rust-analyzer reports at the same file, name and line. The functions rust-analyzer reports that
`map` does not are all inside function bodies (directly, or inside an item declared in a
function body, such as a visitor `impl`) or inside a `const` initializer: the contexts the item
model leaves out.
