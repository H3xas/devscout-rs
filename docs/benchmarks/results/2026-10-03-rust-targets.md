# Rust nested targets, cfg_attr tests and path modules against rust-analyzer — 2026-10-03

Three Rust shapes that rustc compiles and the module model used to place differently: cargo's
nested target files (`tests/<name>/main.rs`, `examples/<name>/main.rs`,
`benches/<name>/main.rs`), test functions marked through `cfg_attr`, and `mod x;` declarations
carrying `#[path = "..."]`. Outside `fixtures/` this repository holds none of them, so they are
judged on the
[`fixtures/rust-targets/`](../../../fixtures/rust-targets/README.md) package, and this
repository's own run guards the registered thresholds. The method is
[methodology.md § Rust edges](../methodology.md#rust-edges); the harness is
[`bench/rust_edges.py`](../../../bench/rust_edges.py).

## Setup

| Item | Value |
|---|---|
| Fixture | `fixtures/rust-targets/`, exported on its own as the tree-ish `<rev>:fixtures/rust-targets` |
| Base revision | `73bfea40add31bb26075834eb3e9fca59cfaa4ad`, mapped by a release build of the same revision |
| Head revision | `92116bcbce8e04754e5fb0aff62966b1d9bf1108`, the last revision that changes code, mapped by a release build of the same revision |
| Fixture revision | `0a8fea572682e4ac4107c8981e3f6aab878b5c61` for the base run; the fixture is unchanged at the head |
| Oracle | `rust-analyzer 1.97.1 (8bab26f4 2026-07-14)`, the toolchain component |
| Toolchain | `rustc 1.97.1 (8bab26f4f 2026-07-14)`, `cargo 1.97.1 (c980f4866 2026-06-30)`, aarch64 macOS |
| Network | none: every cargo and rust-analyzer step ran with `CARGO_NET_OFFLINE=true` and a scratch target directory |

Steps, per run (`<rev>`, `<tree-ish>`, `<label>` and the scratch paths vary):

```sh
git archive <rev> | tar -x -C <scratch>/build
cargo build --release --offline --locked --manifest-path <scratch>/build/Cargo.toml --target-dir <scratch>/target
python3 -B bench/rust_edges.py run --repo . --commit <tree-ish> \
  --devscout <scratch>/target/release/devscout \
  --rust-analyzer "$(rustup which rust-analyzer)" \
  --label <label> --scratch <scratch>/oracle-<label>
```

The fixture runs pass `--commit 0a8fea5:fixtures/rust-targets` with the base build and
`--commit 92116bc:fixtures/rust-targets` with the head build; this repository's run passes
`--commit 92116bc`.

## Which files rustc compiles

The fixture has 24 `.rs` files. rust-analyzer's universe is 15 of them: the library and its
modules, each `#[path]` target, and the three nested targets with their `mod part;` siblings.
The 9 default-rule files a `#[path]` attribute bypasses are outside it, as the rule predicts.
Each candidate file holds a test function with a name of its own, and the compiler's list
names only those of the files the rule predicts:

| Target (`cargo test --all-targets -- --list`) | Compiler | Scan | Differences |
|---|---:|---:|---:|
| `src/lib.rs` | 10 | 10 | 0 |
| `tests/flight/main.rs` | 1 | 1 | 0 |
| `examples/glide/main.rs` | 0 | 0 | 0 |
| `benches/lift/main.rs` | 0 | 0 | 0 |

The library's ten tests include the four `cfg_attr` forms (`checks::flies_under_test`,
`flies_under_any`, `flies_nested`, `flies_ignored`) and one test from each `#[path]` target
(`alias::real_is_compiled`, `hub::hub_is_compiled`, `hub::spoke::spoke_beside_hub_is_compiled`,
`bay::pier::dock_is_compiled`, `reel::cog::cog_is_compiled`,
`reel::spool::line::thread_is_compiled`); no decoy test appears.

## Fixture headline

| Metric | Base | Head |
|---|---:|---:|
| Call-edge population (path calls to a recorded fn) | 31 | 31 |
| Rust `uses-member` edges | 21 | 31 |
| … matching the oracle | 16 | 31 |
| … contradicted (`wrong-target`) | 5 | 0 |
| Call-edge recall | 0.5161 | **1.0** |
| Call-edge precision | 0.7619 | **1.0** |
| Rust `uses-type` edges, all matching | 3 | 6 |
| Population references not recalled (`calls.missed`) | 15 | 0 |
| Test pairs, oracle / devscout / matched | 13 / 7 / 7 | 13 / 13 / 13 |
| Tests, oracle scan / devscout | 11 / 7 | 11 / 11 |

At the base, the 5 contradicted edges are `alias::f()`, `hub::center()`, `bay::pier::moor()`,
`cog::turn()` and `spool::line::wind()`, each bound into the default-rule file rustc does not
compile. The 15 missed references are those 5, `hub::spoke::spin()`, and the 9 calls through
`rust_targets::` from the nested targets and their siblings. The 4 tests devscout did not recognize are the `cfg_attr` ones. At
the head every one of them is recalled and no edge is contradicted.

## This repository at the head

| Metric | Threshold | Head |
|---|---|---:|
| Call-edge population | — | 9,765 (2,932 inside macro arguments) |
| Call-edge recall | ≥ 0.85 | **0.9991** (9,756 / 9,765) |
| Call-edge precision | ≥ 0.98 | **1.0** (9,756 / 9,756) |
| Test-edge recall | ≥ 0.85 | **0.9988** (4,972 / 4,978) |
| Test-edge precision | ≥ 0.98 | **1.0** (4,972 / 4,972) |
| Tests the compiler lists, over 63 targets / the scan / devscout | — | 1,835 / 1,835 / 1,835 |

Every threshold is met and no Rust edge is contradicted: all 9,756 `uses-member` and 6,231
`uses-type` edges match. The one `ambiguous` edge is `AnyFragment::from(...)`, whose candidates
include the oracle's target. The 9 missed calls are that one and 8 calls where a local of the
function's own name shadows the call (`let header = header(...)`, `let text = text(...)`,
`let splice_edges = splice_edges(...)`, `let enclosing = enclosing_method(...)`,
`let verb_overloads = verb_overloads(...)`), the classes the 2026-09-27 Rust edges results name.

Before this change the repository held none of the three shapes, so the change cannot move its
map of that tree: one
`git archive` export of the base revision, mapped cold by the base build and by the head build,
gives byte-identical files.

| File | sha256 (both builds) |
|---|---|
| `graph.json` | `5291841dd23f282812fa31178a80f230caad5cebc98bb163865f6f7d7b62a7cd` |
| `manifest.json` | `d37b7f58a4c5f40eda15caa815619be67a3e0fd972e52732c3855be8f95034e2` |
| `rust-crates.json` | `f9c1fd6e5b35f10c3a9981ace0c2aa8e11b47075b5fe584395cee745837173b7` |

## Where peers win

The forms the model leaves out still reach nothing: a `path` carried by `cfg_attr`, a `mod x;`
inside an inline module that carries `#[path]`, and `src/bin/` targets, which the walk skips.
rust-analyzer resolves all of them, along with everything the 2026-09-27 results list (method
calls typed by their receiver, trait dispatch, macro expansion).
