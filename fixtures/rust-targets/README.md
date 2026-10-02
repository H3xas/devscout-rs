# rust-targets fixture

A kite workshop, invented and neutral: kites, reels, spools and the line that ties them. It is
a real Cargo package with no dependencies, so `cargo check --offline` and
`cargo test --offline --all-targets -- --list` pass on a copy of it. It holds the three shapes
the Rust module model has to place the way rustc does:

- cargo's nested target files `tests/<name>/main.rs`, `examples/<name>/main.rs` and
  `benches/<name>/main.rs`, each a crate root that calls the library through its crate name;
- functions marked as tests through `cfg_attr`;
- `mod x;` declarations carrying `#[path = "..."]`, each beside a default-rule file rustc does
  not compile.

`tests/cli_rust_targets.rs` copies it into a scratch root, maps it there, and pins what `refs`
and `tests` answer on it. `bench/rust_edges.py run --commit <commit>:fixtures/rust-targets`
judges it against rust-analyzer.

Every file a `#[path]` attribute names, and every default-rule file it bypasses, holds a test
function with a name of its own, so the compiler's test list shows which file rustc compiled.

## Layout

| File | Role |
| --- | --- |
| `Cargo.toml` | `[package] name = "rust-targets"`, so the library is `rust_targets`; no `[[test]]`, `[[example]]` or `[[bench]]` entry, so every target is auto-discovered |
| `src/lib.rs` | the library root: the `Kite` type, `fly`, and `launch`, which calls through every `#[path]` module |
| `src/checks.rs` | four functions marked as tests through `cfg_attr`, and one `cfg_attr(test, allow(...))` function that is not a test |
| `src/real.rs` | the file `#[path = "real.rs"] mod alias;` names, at file level of a crate root |
| `src/deep/hub.rs`, `src/deep/spoke.rs` | the file `#[path = "deep/hub.rs"] mod hub;` names, and its own `mod spoke;` resolved beside it |
| `src/bay/dock.rs` | the file `#[path = "dock.rs"]` names inside the inline `bay` module of `src/lib.rs` |
| `src/reel.rs` | a non-mod-rs module holding `#[path = "../gear/cog.rs"] mod cog;` and the inline `spool` module |
| `gear/cog.rs` | the file `../gear/cog.rs` names, relative to `src/reel.rs`'s own directory |
| `src/reel/spool/thread.rs` | the file `#[path = "thread.rs"]` names inside the inline `spool` module of `src/reel.rs` |
| `tests/flight/main.rs`, `tests/flight/part.rs` | a nested integration test and its `mod part;` sibling, which calls back into the test crate through `crate::` |
| `examples/glide/main.rs`, `examples/glide/part.rs` | a nested example and its sibling |
| `benches/lift/main.rs`, `benches/lift/part.rs` | a nested bench and its sibling |

Default-rule files rustc does not compile, each holding a decoy of the same function:

| File | The default rule it would win under |
| --- | --- |
| `src/alias.rs` | `mod alias;` without its attribute |
| `src/hub.rs` | `mod hub;` without its attribute |
| `src/deep/hub/spoke.rs` | `mod spoke;` from a non-mod-rs `src/deep/hub.rs` |
| `src/bay/pier.rs` | `mod pier;` inside `bay` without its attribute |
| `src/dock.rs` | `dock.rs` read from the file's directory instead of the inline module's folder |
| `src/reel/cog.rs` | `mod cog;` from `src/reel.rs` without its attribute |
| `src/gear/cog.rs` | `../gear/cog.rs` read from `src/reel/` instead of `src/` |
| `src/reel/spool/line.rs` | `mod line;` inside `spool` without its attribute |
| `src/spool/thread.rs` | `thread.rs` read as if `src/reel.rs` owned its directory |

## What gives an edge

| Construct | Where | Edge |
| --- | --- | --- |
| `rust_targets::fly()` from each nested target | `tests/flight/main.rs:7`, `examples/glide/main.rs:4`, `benches/lift/main.rs:4` | `uses-member` to `src/lib.rs#fly` |
| `rust_targets::Kite::new()` from each nested target | `tests/flight/main.rs:8`, `examples/glide/main.rs:5`, `benches/lift/main.rs:5` | `uses-type` to `src/lib.rs#Kite`, `uses-member` to `src/lib.rs#Kite::new` |
| a sibling's call through the library name | `tests/flight/part.rs:3`, `examples/glide/part.rs:2`, `benches/lift/part.rs:2` | `uses-member` into `src/lib.rs` |
| a sibling's `crate::` call | `tests/flight/part.rs:2` `crate::helper()` | `uses-member` to `tests/flight/main.rs#helper` |
| `alias::f()` | `src/lib.rs:29` | `uses-member` to `src/real.rs#f` |
| `hub::center()`, `hub::spoke::spin()` | `src/lib.rs:30-31` | `uses-member` to `src/deep/hub.rs#center` and `src/deep/spoke.rs#spin` |
| `bay::pier::moor()` | `src/lib.rs:32` | `uses-member` to `src/bay/dock.rs#moor` |
| `cog::turn()`, `spool::line::wind()` | `src/reel.rs:10-11` | `uses-member` to `gear/cog.rs#turn` and `src/reel/spool/thread.rs#wind` |
| a test file's calls | `tests/flight/main.rs`, `src/checks.rs`, every compiled `#[path]` file | each test function carries its own name in `testMethods` |

## What gives no edge

| Construct | Where | Why |
| --- | --- | --- |
| any call into a default-rule file listed above | none from a compiled file | the `#[path]` declaration names another file, and the default file is never a fallback |
| `not_a_test` | `src/checks.rs` | `cfg_attr(test, allow(dead_code))` carries no test attribute |
