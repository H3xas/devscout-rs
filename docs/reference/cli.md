# CLI reference

Hand-written for now; this page will be generated from the command definitions.

`devscout --help` prints the same command list. Every verb runs against the repository that
contains the current directory, or the one `-C <dir>` names.

## Commands

| Command | What you get |
| --- | --- |
| `devscout init [scope ...]` | Register the repo, create the artifact directory, install the agent hooks, run a first map |
| `devscout map [scope ...] [--no-semantic]` | Build or refresh the index; incremental — unchanged files are reused. `--no-semantic` skips an admitted compiler-facts artifact even when one is present (see [Compiler facts](../guides/compiler-facts.md)) |
| `devscout find <query>` | Search the manifest by symbol name or by file purpose |
| `devscout refs <symbol>` | Inbound references to a symbol, grouped by edge kind (`inherits`, `uses-type`, `uses-member`, `implements`, `overrides`), plus `bus-hop` rows for a handler or for a type whose file publishes a message: possible routes from a recognized publish or dispatch call to a matching handler, runtime routing unverified |
| `devscout read <symbol>` | The symbol's declaration span and verbatim source plus the same inbound answer as `refs` |
| `devscout impact <file\|symbol>` | Blast radius: the files reachable from a seed within N hops |
| `devscout import-edges <file> --repo <id>` | Load a versioned cross-repo edge export; `impact` then reports files reached only through it (`--no-imports` to skip it). See [Cross-repo impact](../guides/cross-repo-impact.md) |
| `devscout compiler-facts run\|import\|status` | Optional: acquire or import a versioned compiler-derived fact artifact (see [Compiler facts](../guides/compiler-facts.md)); `map` and every query never need it and never start it |
| `devscout tests <symbol>` | The test files that reach a symbol |
| `devscout <verb> <symbol> --pick N` | On `refs`, `read`, `impact` and `tests`, narrows a member seed with several declaring types to its nth candidate |
| `devscout <verb> <seed> --no-bus` | On `refs`, `read`, `impact` and `tests`, skips `bus-hop` edges and restores the traversal and hub classification those verbs had before them |
| `devscout stats` | Index and cache summary for the current repo |
| `devscout clear` | Drop freshness rows by age or by session |

## Plumbing verbs

Plumbing verbs (`parse`, `spans`, `extract-dump`, `audit`, `hook`, `noop`) exist for debugging,
for benchmarking and for the agent-hook integration:

| Command | What it does |
| --- | --- |
| `devscout parse <file.cs>` | Dump the parse tree |
| `devscout spans <file.cs>` | Dump declaration spans |
| `devscout extract-dump <file.cs>` | Dump extraction records |
| `devscout audit --semantic <refs.jsonl>` | Score `uses-member` edges against a compiler oracle; see [methodology](../benchmarks/methodology.md#resolver-precision) |
| `devscout hook <read\|bash>` | The agent hook filters, stdin to stdout; see [Agent hooks](../guides/agent-hooks.md) |
| `devscout noop` | Exit 0 (a harness probe) |

The `parse` and `spans` diagnostics show the raw tree, both arms of every preprocessor group
included.

## Global options

| Option | Effect |
| --- | --- |
| `-C <dir>` | Run as if `devscout` started in `<dir>` |
| `-h`, `--help` | Show the command list |
| `-V`, `--version` | Show the version |

## Flags by verb

### init

`devscout init [scope ...] [--label L] [--no-hooks] [--no-map]`

| Flag | Effect |
| --- | --- |
| `scope ...` | Directories to index, relative to the repository root; none means the whole root |
| `--label L` | A label stored with the root in the registry |
| `--no-hooks` | Skip the agent-hook install; see [Agent hooks](../guides/agent-hooks.md) |
| `--no-map` | Register the root without running the first map |

### map

`devscout map [scope ...] [--refresh] [--no-semantic]`

| Flag | Effect |
| --- | --- |
| `scope ...` | Directories to index |
| `--refresh` | Accepted and ignored; every `map` is incremental |
| `--no-semantic` | Skip an admitted compiler-facts artifact and rebuild the syntax-only graph |

### clear

`devscout clear --older-than <days>` drops freshness rows older than N days;
`devscout clear --session <id>` drops one session's freshness rows.

### find

`devscout find <query> [--resources]`

| Flag | Effect |
| --- | --- |
| `--resources` | Include resource keys in the match list; by default they are reduced to a one-line trailer |

### refs, read, impact and tests

```text
devscout refs <symbol>         [--out] [--all] [--no-guess] [--no-dispatch] [--no-bus] [--pick N] [--json|--compact]
devscout read <symbol>         [--no-guess] [--no-dispatch] [--no-bus] [--pick N] [--json|--compact]
devscout impact <file|symbol>  [--hops N] [--no-iface] [--no-guess] [--no-dispatch] [--no-bus] [--no-imports]
                               [--iface-max-fanin N] [--hub-max-indegree N] [--pick N] [--json|--compact]
devscout tests <symbol>        [--no-guess] [--no-dispatch] [--no-bus] [--pick N] [--json|--compact]
```

| Flag | Verbs | Effect |
| --- | --- | --- |
| `--json` | all four | A versioned, machine-readable answer; see [JSON output](json-output.md) |
| `--compact` | all four | One line per hit, for piping; heuristic rows are marked `x` (`ext`) and `h` (`guess`) |
| `--pick N` | all four | Select the nth (one-based) candidate of an ambiguous member seed; an out-of-range `N` is a usage error (exit code 2) |
| `--no-guess` | all four | Drop the `guess` tier, keeping `ext` |
| `--no-dispatch` | all four | Drop the `implements` and `overrides` edges that dependency-injection registrations record |
| `--no-bus` | all four | Skip `bus-hop` edges |
| `--out` | `refs` | Also list outbound references |
| `--all` | `refs` | Lift the inbound and outbound row caps |
| `--hops N` | `impact` | How many hops from the seed to walk (at least 1) |
| `--no-iface` | `impact` | Do not widen through an interface to its implementations' callers |
| `--iface-max-fanin N` | `impact` | Treat an interface injected into more than N constructors as infrastructure and stop widening through it; `0` disables the brake |
| `--hub-max-indegree N` | `impact` | Stop widening through a file that N or more other files reference; `0` disables the in-degree half of the brake |
| `--no-imports` | `impact` | Ignore an imported cross-repo edge export |

The seed spellings these verbs accept are described in [Concepts](../concepts.md#seeds).

### import-edges

`devscout import-edges <file> --repo <id>`; `--repo` is required. See
[Cross-repo impact](../guides/cross-repo-impact.md).

### compiler-facts

```text
devscout compiler-facts run --solution <path.sln|path.csproj> [--target T] [--configuration C] [--platform P]
                            [--capabilities a,b] [--timeout-ms N] [--output-cap-bytes N]
devscout compiler-facts import <file> [--target T] [--configuration C] [--platform P]
devscout compiler-facts status
```

See [Compiler facts](../guides/compiler-facts.md) for what each sub-command admits and reports.

### audit

`devscout audit --semantic <refs.jsonl> [--units F] [--defs F] [--json] [--assert F] [--fp-sites F]`;
see [methodology](../benchmarks/methodology.md#resolver-precision).

### hook

`devscout hook <read|bash>`; see [Agent hooks](../guides/agent-hooks.md).

## Output formats

`refs`, `read`, `impact`, and `tests` also take `--json` (machine-readable) or `--compact` (one
line per hit, for piping). A zero-hit query is reported as a zero hit, not as an error — the tool
never guesses a different symbol on your behalf, and an ambiguous name prints every candidate
instead of picking one. See [JSON output](json-output.md) for the full contract.

A text or `--compact` answer to `refs`, `read`, `impact` and `tests` opens with the **full** count
before any row cap applies: `N inbound (showing M)` on `refs` and `read`, `N files (showing M)` on
`impact` and `tests`. A table that was truncated can therefore never be mistaken for the whole
answer. On `refs`/`read`, N counts the five reference kinds and the four TypeScript kinds (`call`,
`jsx-use`, `dispatch`, `import`) and excludes the `bus-hop` rows, which are possible routes rather
than references; on `impact`, N is the number of files the walk reached, whether or not the row cap
printed them; on `tests`, no row cap applies and the two numbers agree.
A resolved answer that reached nothing states `0 inbound (showing 0)` or `0 files (showing 0)`.
The line belongs to those four verbs and those two formats: `--json` carries the same numbers as
its own table fields instead, `find` has neither the line nor a JSON shape, and an ambiguous,
not-found or fallback answer states no count at all.

A text or `--compact` answer to `refs`, `read`, `tests` and `impact`, and a text answer to `find`
— which has no format flag — ends with a `next:` line naming the cheapest verb to run next:

```text
next: devscout tests Widgetworks.Catalog.InMemoryCatalogStore | devscout impact InMemoryCatalogStore.cs
```

The line is written to be pasted, so an argument a shell would read as its own — a space, `#`,
`$`, a quote, a glob — is single-quoted. Every suggestion was resolved against the verb it names
before it was printed, so each command on the line runs as it stands. An answer with nothing left
to suggest omits the line entirely. Under `--json` the same suggestions arrive as a `next` array of
argv arrays, without the binary name; see [JSON output](json-output.md#next).

## Exit codes

A usage error, such as a missing argument or an out-of-range `--pick N`, exits 2. On `refs`,
`read`, `impact` and `tests` the exit code otherwise follows the answer's outcome, as
[JSON output](json-output.md#outcome) describes; an ambiguous answer exits 1.

## read

`devscout read <symbol>` returns the indexed declaration's start and end lines,
the verbatim source in that span, and its inbound references. `<symbol>` is a
type name or a member seed (see [Seeds](../concepts.md#seeds) — a bare name,
`Type.Member`, or `Namespace.Type.Member`); a member seed answers with its own
declaration line and inbound references, carrying no span (nothing records an
end line for a member on its own). Use `--compact` for a line-oriented summary
or `--json` for structured output. References that originate inside the
target declaration itself are excluded from inbound rows and counts, so
recursive and other self-references do not look like external callers.
