# rust-definitions fixtures

A shelf-and-cabinet workshop, invented and neutral. The files are never compiled: there is
no `Cargo.toml`, `mod drawers;` and `mod fittings;` name files that do not exist, and the
extern block declares an extern type. They exist to be mapped, so `find` and `read` can be
checked against every Rust construct `map` records and every one it leaves out.
`tests/cli_rust_definitions.rs` copies `src/` into a scratch root and maps it there.

## What is recorded

Every item below is one definition (id `<file>#<path>`) and one name row. The span starts at
the first outer doc comment or attribute directly above the item; the name row sits on the
line of the name.

| File | Construct | Kind | Path |
| --- | --- | --- | --- |
| `src/shelf.rs` | `pub struct Widget` under a `///` doc, a plain `//` comment and `#[derive]` | `struct` | `Widget` |
| `src/shelf.rs` | `pub enum Tone` | `enum` | `Tone` |
| `src/shelf.rs` | `pub union Bits` under a doc and `#[repr(C)]` | `union` | `Bits` |
| `src/shelf.rs` | `pub trait Render` under a plain comment and then a doc | `trait` | `Render` |
| `src/shelf.rs` | the trait's `type Canvas`, `const LAYERS`, required `fn draw`, provided `fn outline` | `type`, `const`, `fn`, `fn` | `Render::Canvas`, `Render::LAYERS`, `Render::draw`, `Render::outline` |
| `src/shelf.rs` | `pub type Shade`, `pub fn polish`, `pub const SHELF_DEPTH`, `pub static SHELF_LABEL` | `type`, `fn`, `const`, `static` | the name |
| `src/shelf.rs` | `macro_rules! stack_of` | `macro_rules` | `stack_of` |
| `src/shelf.rs` | `mod drawers;` | `mod` | `drawers` |
| `src/shelf.rs` | `impl Widget`: `MAX_SPINE`, `new`, `spine` | `const`, `fn`, `fn` | `Widget::MAX_SPINE`, `Widget::new`, `Widget::spine` |
| `src/shelf.rs` | `impl Render for Widget`: `Canvas`, `LAYERS`, `draw` | `type`, `const`, `fn` | `Widget::Canvas`, `Widget::LAYERS`, `Widget::draw` |
| `src/shelf.rs` | `pub struct Gadget`, then `fmt` in `impl fmt::Display` and in `impl fmt::Debug` | `struct`, `fn`, `fn` | `Gadget`, `Gadget::fmt`, `Gadget::fmt-1` |
| `src/shelf.rs` | `home_shelf` under `#[cfg(unix)]` and again under `#[cfg(not(unix))]` | `fn`, `fn` | `home_shelf`, `home_shelf-1` |
| `src/shelf.rs` | `pub fn r#match` | `fn` | `match` |
| `src/shelf.rs` | `const PLANK` | `const` | `PLANK` |
| `src/cabinet.rs` | inline `pub mod cabinet`, and inside it every kind again: `Hinge`, `Swing`, `Latch`, `Open` with its `open`, `Screw`, `assemble`, `CABINET_WIDTH`, `CABINET_MAKER`, `twice`, `mod fittings;` | each item's own kind | `cabinet::<name>`, `cabinet::Open::open` |
| `src/cabinet.rs` | `impl Hinge { fn oil }` and `impl Open for Hinge { fn open }` inside the module | `fn`, `fn` | `cabinet::Hinge::oil`, `cabinet::Hinge::open` |
| `src/cabinet.rs` | `extern "C"` block: `shelf_count`, `SHELF_ERRNO`, `ShelfHandle` | `fn`, `static`, `type` | the name (an extern block adds no segment) |
| `src/tones.rs` | `pub fn default_tone`, `pub const TONE_COUNT` | `fn`, `const` | the name |

## What is not recorded

| File | Construct | Why |
| --- | --- | --- |
| `src/shelf.rs` | `fn tucked_helper` inside `polish`'s body | a function body is never walked; the item has no path of its own |
| `src/shelf.rs` | `fn hidden_in_const` inside `PLANK`'s initializer | an initializer is never walked |
| `src/shelf.rs` | `fn made_by_macro` inside the `macro_rules!` body | a token tree is never walked |
| `src/shelf.rs` | `static TUCKED_COUNTER` inside `thread_local!` | a macro invocation is a token tree |
| `src/shelf.rs` | the `Crimson` and `Slate` variants, the `spine_width` field | enum variants and struct fields are not items |
| `src/shelf.rs` | `use ... as ShelfMap` | a `use` declaration is not an item |
| `src/shelf.rs` | `const _` | an item named `_` has no name to find it by |
| both | every `impl` block | an `impl` block has no name; its items carry the self type as their owner segment |
| `src/prelude.rs` | two `pub use` lines | no item at all |

## Purposes

| File | Purpose rule | Source |
| --- | --- | --- |
| `src/shelf.rs` | the `//!` line, ` — `, then the file-level items grouped by kind, cut at 200 UTF-16 units | `ast` |
| `src/cabinet.rs` | the leading plain `//` comment, ` — `, then `mod cabinet` | `ast` |
| `src/tones.rs` | first node is a `use`, so the grouped items alone | `ast` |
| `src/prelude.rs` | no item and no leading comment, so the heuristic text | `ast-none` |

## Line rules

- `src/shelf.rs` `Widget`: the span starts at the `///` doc, passes over the plain comment
  between it and `#[derive]`, and ends at the closing brace.
- `src/shelf.rs` `Render`: the plain comment above the doc is not part of the span.
- `src/cabinet.rs` `Hinge`: the span starts at its `///` doc and stops before the module's
  `//!` inner doc directly above it.
- `src/cabinet.rs` `cabinet`: the span starts at the module's doc; the file's leading plain
  comment is not part of it.
- `src/shelf.rs` `SHELF_LABEL`: a letter outside the Basic Multilingual Plane in its doc
  leaves every later line number unchanged.
