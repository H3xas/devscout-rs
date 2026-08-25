# devscout

A fast code index for C# and TypeScript codebases — `map`, `find`, `refs`, `impact` from the CLI.

`devscout` walks a repository once, parses it with [tree-sitter](https://tree-sitter.github.io/)
(C#, TypeScript, TSX, JavaScript), and writes a small set of JSON artifacts next to the repo.
Every later query is answered from those artifacts, so asking "who calls this?" or "what breaks
if I touch this file?" costs a few milliseconds instead of a full-text sweep.

It is built for the case where a person or an agent needs a *name-level* answer — a definition
site, an inbound edge, a blast radius — and grep would return either nothing useful or far too
much.

## What it does

| Command | What you get |
| --- | --- |
| `devscout init [scope ...]` | Register the repo, create the artifact directory, install the agent hooks, run a first map |
| `devscout map [scope ...]` | Build or refresh the index; incremental — unchanged files are reused |
| `devscout find <query>` | Search the manifest by symbol name or by file purpose |
| `devscout refs <symbol>` | Inbound references to a symbol, grouped by edge kind (`inherits`, `uses-type`, `uses-member`) |
| `devscout impact <file\|symbol>` | Blast radius: the files reachable from a seed within N hops |
| `devscout tests <symbol>` | The test files that reach a symbol |
| `devscout stats` | Index and cache summary for the current repo |
| `devscout clear` | Drop freshness rows by age or by session |

Plumbing verbs (`parse`, `spans`, `extract-dump`, `hook`, `noop`) exist for debugging and for
the agent-hook integration. `devscout --help` lists everything.

**Languages.** C# (`.cs`) is the complete story: declarations, inheritance, type and member
usage, and preprocessor-aware extraction. TypeScript / TSX / JavaScript (`.ts`, `.tsx`, `.js`,
`.jsx`) are indexed for `find` and file purposes, and are resolved into the graph with a
narrower set of edge kinds — see [Limitations](#limitations).

## Install

From crates.io:

```sh
cargo install devscout-rs
```

The crate is named `devscout-rs`; the binary it installs is `devscout`.

From source, clone this repository and build it:

```sh
cargo build --release
# binary at ./target/release/devscout
```

Pre-built binaries for Linux, macOS, and Windows are attached to each
[tagged release](#versioning-and-releases).

## Quickstart

Any repository with C# or TypeScript in it will do. Using this repo's own fixtures as a
throwaway example:

```sh
mkdir -p /tmp/demo/src && cd /tmp/demo
cp <path-to-this-repo>/fixtures/ts-grammar/* src/
cp <path-to-this-repo>/fixtures/csharp-demo/src/* src/

devscout init
```

`init` registers the root, reports what it found, offers the agent hooks, and runs a first map:

```
devscout initialized at /private/tmp/demo/.scout (non-git root: /private/tmp/demo)
languages: 3 .cs (supported); 1 .js, 9 .ts, 1 .tsx (present, not yet supported)
hooks: installed (Read, Bash); backup /tmp/devscout-home/.claude/settings.json.bak.20260824-074758-728335000
map: mapped 14 files under . (preserved 0 agent purposes, downgraded 0 changed, 14 new, 0 removed, 14 ast signatures); graph rebuilt in 0.01s (24 defs, 4 edges)
```

(macOS resolves `/tmp` to `/private/tmp`; the backup path reflects whatever `$HOME` the hooks
install ran against.)

Then query it:

```
$ devscout find Article
src/ArticleCard.tsx:1: function ArticleCard | interface ArticleCardProps
src/articleTypes.ts:1: interface ArticleItem | interface ArticleAuthor | type ArticleItemStatus | type ArticlePage

$ devscout refs IOrderRepository
Shop.Data.IOrderRepository  (interface)
def: src/IOrderRepository.cs:3
inbound:
  inherits (1):
    src/OrderRepository.cs:3  inherits  public class OrderRepository : IOrderRepository
  uses-type (0):
  uses-member (0):

$ devscout impact src/OrderRepository.cs --hops 2
impact: src/OrderRepository.cs  (file, seed files: src/OrderRepository.cs)  hops<=2
affected files: 1  shown: 1  dropped: 0
file  hops  via  top-symbols
src/OrderController.cs  1  3  OrderRepository
```

`refs`, `impact`, and `tests` also take `--json` (machine-readable) or `--compact` (one line per
hit, for piping). A zero-hit query is reported as a zero hit, not as an error — the tool never
guesses a different symbol on your behalf, and an ambiguous name prints every candidate instead
of picking one.

Re-run `devscout map .` after edits; it re-parses only what changed and leaves the graph alone
when nothing moved (`... 0 new, 0 removed ...; graph unchanged`). If the index falls behind
`HEAD`, queries print a staleness warning on stderr rather than silently answering from stale
data.

## Where it stores things

Inside a git repository, artifacts live under the git common directory, so they never show up as
untracked files and are shared correctly by worktrees:

```
<git-common-dir>/scout/manifest.json              file -> purpose + symbol index
<git-common-dir>/scout/index-state.json           HEAD + timestamp the index was built at
<git-common-dir>/scout/graph/graph.json           definitions and edges
<git-common-dir>/scout/graph/fragments-v13.json   per-file extraction cache (incremental map)
```

Outside a git repository the same tree is written to `<root>/.scout/` instead. `devscout init`
also adds `.scout` to the repository's local exclude file so the legacy location cannot be
committed by accident.

Two stores live outside the repo:

- **Registry** — `$HOME/.claude/scout/repos.json`, the list of roots `devscout` knows about.
- **Read-freshness cache** — `<root>/.scout/cache.db` plus a content-addressed `content.db`,
  both SQLite. These are only written by the agent hooks (`devscout hook read|bash`); plain CLI
  use does not touch them.

## Environment variables

| Variable | Effect |
| --- | --- |
| `SCOUT_REGISTRY` | Path to the registry JSON. Default `$HOME/.claude/scout/repos.json`. |
| `SCOUT_CONTENT_DB` | Path to the shared content-dedup SQLite database. **Set this if you use the hooks** — the default is derived at compile time and is not relocation-safe. |
| `SCOUT_MTIME_REUSE` | `1` switches `map` from content-hash fragment reuse back to mtime-based reuse. |
| `SCOUT_DEBUG` | `1` turns on hook debug output. Equivalent to creating a `.scout/debug` file. |
| `HOME` | Used to locate the registry and the agent settings file. |

## Agent hooks

`devscout init` merges two `PostToolUse` entries into the agent settings file at
`$HOME/.claude/settings.json`, backing up the existing file first: `devscout hook read` and
`devscout hook bash`. They read a tool result on stdin and, when the same content has already
been read in the session, replace the payload with a one-line marker instead of repeating it.
Skip this with `devscout init --no-hooks`; the hook install is independent of the index, and a
failure there never fails `init`.

## Limitations

Known, rather than hidden:

- **TypeScript reference queries fold fewer edge kinds than extraction records.** The TS/TSX
  extractor records more relationships than the graph currently turns into queryable edges, so
  `refs`/`impact` over TypeScript are narrower than over C#.
- **Generic-delegate `typeParams` divergence is under review.** Type-parameter handling for
  generic delegate declarations does not yet agree with the rest of the generic ladder; the
  affected shapes are under review rather than pinned.
- **`init`'s language census understates TypeScript.** It labels every non-`.cs` extension
  "present, not yet supported" because only C# gets the full extraction path. TypeScript is
  indexed and graphed regardless; the census line has not caught up.
- **The content-store fallback path is compile-time derived.** See `SCOUT_CONTENT_DB` above.
- **No watch mode.** `map` is fast and incremental, but you run it; nothing watches the
  filesystem for you.
- **Ordering is load-bearing but not a stability promise.** Artifact ordering is fixed and
  deterministic by design; do not rely on it staying byte-identical across minor versions.

`devscout` began as the Rust half of a two-implementation tool, and a number of source comments
still describe behaviour by reference to that original implementation. Those notes are history:
this crate generates and reads its own artifacts, and interoperating with anything else is
optional.

## Benchmarks

Every claim this project makes about speed, cost, or accuracy lives in
[`docs/benchmarks/`](docs/benchmarks/README.md), with the command that produced it and the
corpus SHA it ran against. The methodology, the peer tools an agent could install instead, the
agentic-lane protocol, and the dated result documents are separate files there, and the harness
is in [`bench/`](bench/README.md).

**Scorecard** (devscout 0.2.0 vs the `rg` baseline, MassTransit corpus; full numbers, per-cell
commands, and the preliminary-run caveats are in
[`docs/benchmarks/results/2026-08.md`](docs/benchmarks/results/2026-08.md)):

| Kind | devscout | rg | Verdict |
| --- | --- | --- | --- |
| Locate | 2/2 correct | 2/2 correct, ~2x faster | Tie — use rg |
| References | 2/2 correct (needs `--all`) | 2/2 correct | Tie, cost mixed |
| Impact | 2/2 partial, better precision, 1 call | 2/2 partial, 4-call chain | devscout wins |
| End-to-end retrieval | 1/2 correct | 2/2 correct | rg wins |
| Agentic, Opus (preliminary) | 4/4 correct, median 180k tokens | 3/4 correct + 1 partial, median 199k tokens | No correctness edge; ~25k-token saving only |

A separate scripted-lane run measured **tool calls issued per task**: the index arm used fewer
calls in all four query kinds, largest on references (5.0 vs 11.8 per lane, ~2.4x) — single-run
proxy, details under "Tool-call proxy" in the dated results.

Gaps are published in both directions. `devscout` answers name-level and reachability questions
from a prebuilt graph; questions that reduce to finding one distinctive string are answered
well and cheaply by a skilled agent holding `rg`, with no index at all. Where a peer or the
plain `rg` baseline leads, the results say so. The dated results in
[`docs/benchmarks/results/2026-08.md`](docs/benchmarks/results/2026-08.md) now include the first
agentic (model-in-the-loop) round — preliminary, one run per cell, its integrity caveats leading.

## Versioning and releases

Semantic versioning. Releases are cut by pushing a `v*` tag (`v0.1.0`, `v0.2.0`, …), which builds
and attaches binaries for Linux, macOS, and Windows. While the version is `0.x`, minor bumps may
change artifact layout — delete the artifact directory and re-run `devscout map` after upgrading.

**Provenance.** devscout is developed alongside a private reference implementation of the same
graph contract; every release is gated on behavioral parity, and the committed test fixtures pin
that contract byte-for-byte in this repository. You never need the reference implementation —
everything required to build, test, and verify devscout is here.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
this crate by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
