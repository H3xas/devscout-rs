# Rust edges against rust-analyzer — 2026-09-27

Precision and recall of devscout's Rust call edges and test-to-function pairs against the
references `rust-analyzer lsif` writes for this repository. The method is
[methodology.md § Rust edges](../methodology.md#rust-edges); the harness is
[`bench/rust_edges.py`](../../../bench/rust_edges.py).

## Setup

| Item | Value |
|---|---|
| Corpus | this repository, exported with `git archive` |
| Run 0 revision | `911fe479190906bc965635f96839592f8459d67e` (Rust definitions, no Rust edges), mapped by a release build of the same revision |
| Head revision | `6f73c09` (Rust edges), mapped by a release build of the same revision |
| Oracle | `rust-analyzer 1.97.1 (8bab26f4 2026-07-14)`, the toolchain component |
| Toolchain | `rustc 1.97.1 (8bab26f4f 2026-07-14)`, `cargo 1.97.1 (c980f4866 2026-06-30)`, aarch64 macOS |
| Network | none: every cargo and rust-analyzer step ran with `CARGO_NET_OFFLINE=true` and a scratch target directory |

Steps, per run (`<rev>`, `<label>` and the scratch paths vary):

```sh
git archive <rev> | tar -x -C <scratch>/build
cargo build --release --offline --locked --manifest-path <scratch>/build/Cargo.toml --target-dir <scratch>/target
python3 -B bench/rust_edges.py run --repo . --commit <rev> \
  --devscout <scratch>/target/release/devscout \
  --rust-analyzer "$(rustup which rust-analyzer)" \
  --label <label> --scratch <scratch>/oracle-<label>
```

Run 0 was rescored from its saved files once, after the site classifier learned the operator,
literal, lifetime and whitespace positions rust-analyzer resolves; the oracle was not re-run.

## Headline

Thresholds were registered before Run 0.

| Metric | Threshold | Run 0 | Head |
|---|---:|---:|---:|
| Call-edge population (path calls to a recorded fn) | — | 8,367 | 8,694 |
| … of which inside macro arguments | — | 2,445 | 2,571 |
| Rust `uses-member` edges | — | 0 | 8,686 |
| Call-edge recall | ≥ 0.85 | 0.0 | **0.9991** (8,686 / 8,694) |
| Call-edge precision | ≥ 0.98 | undefined | **1.0** (8,686 / 8,686) |
| Test pairs, oracle | — | 4,203 | 4,312 |
| Test pairs, devscout | — | 0 | 4,307 |
| Test-edge recall | ≥ 0.85 | 0.0 | **0.9988** (4,307 / 4,312) |
| Test-edge precision | ≥ 0.98 | undefined | **1.0** (4,307 / 4,307) |

Every threshold is met at the head. No Rust edge of any kind is contradicted: all 8,686
`uses-member` and 5,763 `uses-type` edges have an oracle reference at their file and line to
their target. The head carries one `ambiguous` edge, and the oracle's target is among its
candidates.

## Where the head misses

| Miss | Count | Class |
|---|---:|---|
| `let text = text(...)`, `let splice_edges = splice_edges(...)`, `let enclosing = enclosing_method(...)`, `let verb_overloads = verb_overloads(...)` | 7 calls, 4 test pairs | a local of the function's own name shadows the call: locals are flow-insensitive over the body by design, which can drop a call but never binds a local to an item |
| `AnyFragment::from(...)` in `src/graph/tests/fragment_types.rs` | 1 call, 1 test pair | three `From` impls give `AnyFragment` three `from` functions; devscout records one `ambiguous` edge whose candidates include the oracle's target, rather than dispatching on the argument type |

## Tests

| Check | Run 0 | Head |
|---|---:|---:|
| `#[test]` functions, the script's scan | 1,503 | 1,578 |
| Tests the compiler lists (`cargo test --all-targets -- --list`), over all targets | 1,503 (58 targets) | 1,578 (59 targets) |
| Targets where the compiler's list and the scan differ | 0 | 0 |
| Tests devscout recognizes (`testMethods`) | 0 | 1,578 |
| Differences between devscout's tests and the scan | 1,503 (none recognized yet) | 0 |

## Definitions

| Check | Run 0 | Head |
|---|---:|---:|
| Item-shaped LSIF definitions in the universe | 3,925 | 4,174 |
| … mapping to a devscout def by file, name and name line | 3,876 | 4,125 |
| … not mapping | 49 | 49 |

All 49 unmapped definitions are items declared inside function bodies (22 `fn`, 20 `const`, 2
`static`, 2 `struct`, 2 `type`, 1 `macro_rules!`), which the item model does not record.

## Oracle references by class

The call population is the first two rows; every other row is an excluded class.

| Class | Run 0 | Head |
|---|---:|---:|
| population: path call to a recorded fn, macro argument | 2445 | 2571 |
| population: path call to a recorded fn, plain | 5922 | 6123 |
| `use` tree | 3937 | 4165 |
| attribute or derive | 2688 | 2889 |
| macro name | 7500 | 7795 |
| method call to a recorded fn | 1154 | 1250 |
| method call to an external or unrecorded item | 17292 | 18496 |
| operator, literal, lifetime or whitespace | 12501 | 13318 |
| other reference to a recorded item (recorded fn) | 123 | 125 |
| other reference to a recorded item (recorded other) | 837 | 858 |
| other reference to a recorded item (recorded type) | 5847 | 6278 |
| other reference to an external item | 13940 | 15047 |
| other reference to an unrecorded item (local, field, variant) | 42197 | 45291 |
| path call to a recorded type (tuple-struct constructor) | 104 | 104 |
| path call to an external item | 5112 | 5456 |
| path call to an unrecorded item (variant, local, function-body item) | 1141 | 1197 |
| **all oracle references** | 122740 | 130963 |

No reference falls inside a `macro_rules!` body: rust-analyzer resolves nothing there. Universe:
304 files at Run 0 and 325 at the head (every `src/` and `tests/` file and
`examples/extension_impact.rs`); `fixtures/` and `bench/` are reached by no target and excluded.

## Where peers win

rust-analyzer resolves what this pass leaves out by design: method calls typed by their receiver
(1,250 at the head reach a recorded function), trait dispatch, calls macro expansion creates, and
the one `From::from` overload set above. An agent holding rust-analyzer gets those answers;
devscout answers path calls, type uses and tests from a map built in under a second, with no
build of the crate.
