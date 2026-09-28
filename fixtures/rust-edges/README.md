# rust-edges fixture

A lamp workshop, invented and neutral: lamps, wicks, shelves and the polish that keeps them
bright. It is a real Cargo package with no dependencies, so `cargo check --offline` and
`cargo test --offline -- --list` pass on a copy of it. `tests/cli_rust_edges.rs` copies it into
a scratch root, maps it there, and pins what `refs`, `tests` and `impact` answer on it.

## Layout

| File | Role |
| --- | --- |
| `Cargo.toml` | `[package] name = "rust-edges"`, so the library is `rust_edges`; one `[[test]]` whose `path` is `checks/rim.rs`, a root only that key names |
| `src/lib.rs` | the library root: `mod` declarations, an inline `tools` module with an out-of-line child, two `pub use` re-exports, the `Shine` trait, and an inline `#[cfg(test)]` module |
| `src/main.rs` | the binary root, calling the library through `rust_edges::` |
| `src/lamp.rs`, `src/lamp/wick.rs` | a module in the `foo.rs` with `foo/` layout |
| `src/shelf/mod.rs`, `src/shelf/dust.rs` | a module in the `mod.rs` layout |
| `src/tools/polish.rs` | the out-of-line child of the inline `tools` module |
| `src/glow.rs` | `impl Shine for Lamp`, `impl fmt::Display for Lamp` and an inherent `impl Lamp`, all in a file other than the one that defines `Lamp` |
| `tests/wiring.rs` | an integration test calling through `rust_edges::`, declaring the helper module `tests/common/mod.rs` |
| `checks/rim.rs`, `checks/helpers.rs` | the `[[test]]` root and its helper module |

## What gives an edge

| Construct | Where | Edge |
| --- | --- | --- |
| a call through a `mod x;` path | `src/lamp.rs:10` `wick::trim(level)` | `uses-member` to `src/lamp/wick.rs#trim` |
| a call through a `pub use` re-export | `checks/rim.rs:12` `rust_edges::trim(20)` | `uses-member` to `src/lamp/wick.rs#trim` |
| a call inside `vec!` through a glob of the parent | `src/lib.rs:44` `vec![trim(4)]` | `uses-member` to `src/lamp/wick.rs#trim` |
| a call inside `assert_eq!` | `src/lib.rs:43` `light(1)` and `tools::buff(1)` | `uses-member` to each |
| a call inside `matches!`, with a qualified argument | `src/lib.rs:45` `shelf::count(&Shelf::default())` | `uses-member` to `count`, `uses-type` to `Shelf` |
| a call inside `write!` through a type | `src/glow.rs:14` `Lamp::brightness(self)` | `uses-type` to `Lamp`, `uses-member` to `Lamp::brightness` |
| a `super::super::` path | `src/lamp/wick.rs:6` | `uses-member` to `src/lib.rs#light` |
| a `use super::buff` import | `src/tools/polish.rs:4` | `uses-member` to `src/lib.rs#tools::buff` |
| `Self::scale` | `src/glow.rs:20` | `uses-member` to `src/glow.rs#Lamp::scale` |
| a return type, a struct literal, an `impl` self type, a field type | `src/lamp.rs:7-9`, `src/glow.rs:6,12,18`, `src/shelf/mod.rs:5` | `uses-type` to `Lamp` |
| `impl Shine for Lamp` | `src/glow.rs:6` | `implements` to `src/lib.rs#Shine` |
| a test file's calls | `checks/rim.rs`, `tests/wiring.rs`, the inline test module | each `#[test]` function carries its own name in `testMethods` |

## What gives no edge

| Construct | Where | Why |
| --- | --- | --- |
| `lamp.beam()` | `src/lib.rs:25` | a method call is never typed by its receiver |
| `light(level)` in `dim` | `src/lib.rs:30` | the closure named `light` shadows the function |
| `T::default()` | `src/lib.rs:34` | a generic parameter binds nothing |
| `std::cmp::max` | `src/lib.rs:30` | std binds nothing |
| `impl fmt::Display for Lamp` | `src/glow.rs:12` | the trait is not a recorded item; only the self type gives `uses-type` |
| every `use` declaration | throughout | a `use` binds a name and gives no edge |
| `tests/common/mod.rs`, `checks/helpers.rs` | helper modules | no `#[test]` function, so no test file |
