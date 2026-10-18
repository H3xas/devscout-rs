# devscout

[![crates.io](https://img.shields.io/crates/v/devscout-rs.svg)](https://crates.io/crates/devscout-rs)
[![CI](https://github.com/H3xas/devscout-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/H3xas/devscout-rs/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

A code index for C#, TypeScript and Rust repositories. Build it once, then ask who references a
symbol, what a change can break, and which tests reach it. Every answer row names the source
line and the rule that produced it.

devscout parses the repository with tree-sitter and writes a graph of definitions and
references next to it. Queries read that graph. Indexing and queries never run your build and
never touch the network.

## What it answers

| Question | Command |
| --- | --- |
| Where is `Order` defined, and which files are about orders? | `devscout find Order` |
| Who references this type or member? | `devscout refs IOrderRepository` |
| Show the declaration and everything that points at it | `devscout read OrderService.Place` |
| What can break if I change this file or symbol? | `devscout impact src/OrderRepository.cs` |
| Which tests reach this symbol? | `devscout tests OrderService` |
| Give another tool every fact about this repository | `devscout export` |

Add `--json` for a versioned, machine-readable answer
([JSON output](docs/reference/json-output.md)). Every command and flag:
[CLI reference](docs/reference/cli.md).

## Install

```sh
cargo install devscout-rs --locked   # installs the `devscout` binary
```

Signed binaries for Linux, macOS and Windows are on the
[releases page](https://github.com/H3xas/devscout-rs/releases).
[Verify a download](docs/install.md#verify-a-download), or build from source as
[Install](docs/install.md) describes.

## Quickstart

Run these from the root of any repository with C#, TypeScript or Rust in it. The output below
comes from a small sample of three C# files and eleven TypeScript and JavaScript files:

```console
$ devscout init --no-hooks
devscout initialized at ...
languages: ...
hooks: skipped (--no-hooks)
map: mapped 14 files under . (preserved 0 agent purposes, downgraded 0 changed, 14 new, 0 removed, 14 ast signatures); graph rebuilt in ... (30 defs, 7 edges)

$ devscout refs IOrderRepository
Shop.Data.IOrderRepository  (interface)  1 inbound (showing 1)
def: src/IOrderRepository.cs:3
inbound:
  inherits (1):
    src/OrderRepository.cs:3  inherits  public class OrderRepository : IOrderRepository
  uses-type (0):
  uses-member (0):
next: devscout tests Shop.Data.IOrderRepository | devscout impact src/IOrderRepository.cs

$ devscout impact src/OrderRepository.cs --hops 2
impact: src/OrderRepository.cs  (file, seed files: src/OrderRepository.cs)  hops<=2  1 files (showing 1)
affected files: 1  shown: 1  dropped: 0
file  hops  via  top-symbols
src/OrderController.cs  1  3  OrderRepository
next: devscout refs Shop.Data.OrderRepository
```

A resolved `refs`, `read`, `impact` or `tests` answer in text or `--compact` mode opens with the
full count before any cap applies (`1 inbound (showing 1)`), and a text answer from those verbs or
from `find` ends with a `next:` line naming the cheapest verb worth running — omitted when nothing
is left to suggest — so a long answer tells you both what you are not seeing and where to go next.
Under `--json` the totals arrive as table fields and the suggestions as a `next` array; `find` has
no `--json` shape.

Run `devscout map` after edits; only changed files are re-parsed. An ambiguous name lists every
candidate: `--pick N` answers the nth one, and `--pick all` answers each in its own section
([Concepts](docs/concepts.md#seeds)).

## Language support

**C#** (`.cs`) is the complete story: definitions, inheritance, type and member references,
implementations registered through dependency injection, and candidate message-bus routes, with
optional compiler-verified enrichment ([Compiler facts](docs/guides/compiler-facts.md)). Which
constructs produce facts is catalogued in [C# constructs](docs/reference/csharp-constructs.md),
and the targets the compiler-fact path is qualified on in
[.NET targets](docs/reference/dotnet-targets.md).

**TypeScript and JavaScript** (`.ts`, `.tsx`, `.mts`, `.cts`, `.js`, `.jsx`, `.mjs`, `.cjs`) are
indexed for `find` and file purposes and graphed with imports (tsconfig `paths` and re-export
barrels), calls, JSX usage and dispatch. `refs`, `read`, `impact` and `tests` read those edges for
TypeScript and JavaScript alike. File-scope declarations, including declarations in a top-level
IIFE body, are indexed even without an export. Their references resolve within the same file only;
an exported definition wins a bare-name tie. JavaScript query quality is not measured.

**Angular templates** (`.html`) are read for the components that name them. Each member of an
exported `@Component({...})` class gets a name row. A member read in the component's
`templateUrl` file or inline `template` becomes a `uses-member` edge from the template line, so
`refs <member>` lists the template lines that read it. TypeScript member reads (`this.x` in a
class) are not indexed, and the `refs` answer prints a note that says so
([Angular templates](docs/explanation/resolution.md#angular-templates)).

**Rust** (`.rs`) is indexed and graphed through path resolution: modules, `use` scope, calls,
type uses, trait impls and `#[test]` functions, with no method-call typing
([Rust constructs](docs/reference/rust-constructs.md)).

Every known gap, one line each: [Limitations](docs/limitations.md).

## When to use it, and when not

- **Use devscout** for name-level and reachability questions: implementations of an
  interface, callers of a member through a base type, the blast radius of a file.
- **Use `rg`** to find a distinctive string. It is as correct and faster there; the
  [benchmarks](docs/benchmarks/README.md) publish where each tool wins.
- **Candidates are labelled.** Heuristic rows carry a `tier`; message-bus rows are possible
  routes, not verified ones. `--no-guess` and `--no-bus` remove them.
- **No silent guessing.** An ambiguous name lists every candidate, and an empty answer is
  reported as empty.

## Agent hooks

Optional. `devscout init` without `--no-hooks` backs up the agent settings file
(`~/.claude/settings.json`), then adds two `PostToolUse` hooks that replace a repeated file read
with a one-line marker. The index works the same without them.
[What they do and how to remove them](docs/guides/agent-hooks.md).

## Documentation

| I want to | Read |
| --- | --- |
| Find the right page | [Documentation map](docs/README.md) |
| Understand the graph, edges, tiers and seeds | [Concepts](docs/concepts.md) |
| Look up a command, flag or variable | [Reference](docs/README.md#reference) |
| Enrich C# with compiler-verified facts | [Compiler facts](docs/guides/compiler-facts.md) |
| Read the whole fact set as a versioned stream | [Fact export](docs/reference/fact-export.md) |
| Connect impact across repositories | [Cross-repo impact](docs/guides/cross-repo-impact.md) |
| Upgrade from an earlier version | [Upgrading](docs/guides/upgrading.md) |
| Fix a stale, empty or ambiguous answer | [Troubleshooting](docs/guides/troubleshooting.md) |
| See the evidence behind accuracy claims | [Benchmarks](docs/benchmarks/README.md) |
| Contribute | [CONTRIBUTING.md](CONTRIBUTING.md), [ARCHITECTURE.md](ARCHITECTURE.md) |

## Project status

devscout is pre-1.0 and follows semantic versioning; releases are cut from `v*` tags, and a
minor release may change the artifact layout ([Upgrading](docs/guides/upgrading.md)). Changes:
[CHANGELOG.md](CHANGELOG.md). Security reports: [SECURITY.md](SECURITY.md). How the project is
run: [GOVERNANCE.md](GOVERNANCE.md), [MAINTAINERS.md](MAINTAINERS.md) and the
[Code of Conduct](CODE_OF_CONDUCT.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion
in this crate by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
