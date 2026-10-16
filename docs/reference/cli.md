# CLI reference

> **Generated.** Every byte of this page is written by `tests/docs_sync.rs` from
> the definitions the binary itself uses, and the page is never edited by hand: a change that
> belongs here belongs in the source. Regenerate it with `BLESS=1 cargo test --test docs_sync`,
> and resolve a merge conflict in it by regenerating rather than by merging edits.

`devscout --help` prints the same command list. Every verb runs against the repository that contains the current directory, or the one `-C <dir>` names.

## Commands

| Command | What you get |
| --- | --- |
| `devscout init [scope ...]` | Register the repo, create the artifact directory, install the agent hooks, run a first map |
| `devscout map [scope ...]` | Build or refresh the index; incremental — unchanged files are reused. `--no-semantic` skips an admitted compiler-facts artifact even when one is present (see [Compiler facts](../guides/compiler-facts.md)) |
| `devscout find <query>` | Search the manifest by symbol name or by file purpose |
| `devscout refs <symbol>` | Inbound references to a symbol, grouped by edge kind (`inherits`, `uses-type`, `uses-member`, `implements`, `overrides`), plus `bus-hop` rows for a handler or for a type whose file publishes a message: possible routes from a recognized publish or dispatch call to a matching handler, runtime routing unverified |
| `devscout read <symbol>` | The symbol's declaration span and verbatim source plus the same inbound answer as `refs` |
| `devscout impact <file\|symbol>` | Blast radius: the files reachable from a seed within N hops |
| `devscout import-edges <file>` | Load a versioned cross-repo edge export; `--repo` is required, because the repository id is never inferred. `impact` then reports files reached only through it. See [Cross-repo impact](../guides/cross-repo-impact.md) |
| `devscout compiler-facts <run\|import\|status>` | Optional: acquire or import a versioned compiler-derived fact artifact (see [Compiler facts](../guides/compiler-facts.md)); `map` and every query never need it and never start it |
| `devscout export` | Write the current repository's whole fact set as one versioned NDJSON stream — the contract downstream tools read instead of querying symbol by symbol. See [Fact export](fact-export.md) |
| `devscout tests <symbol>` | The test files that reach a symbol |
| `devscout stats` | Index and cache summary for the current repo |
| `devscout clear` | Drop freshness rows by age or by session |

An unrecognized command prints this line and exits 1:

```text
usage: devscout <noop|parse|spans|extract-dump|hook|refs|read|impact|tests|find|import-edges|compiler-facts|export|map|stats|clear|init> [args]
```

## Plumbing verbs

Plumbing verbs exist for debugging, for benchmarking and for the agent-hook integration:

| Command | What it does |
| --- | --- |
| `devscout parse <file.cs>` | Dump the parse tree |
| `devscout spans <file.cs>` | Dump declaration spans |
| `devscout extract-dump <file.cs>` | Dump extraction records |
| `devscout audit` | Score `uses-member` edges against a compiler oracle; see [methodology](../benchmarks/methodology.md#resolver-precision) |
| `devscout hook <read\|bash>` | The agent hook filters, stdin to stdout; see [Agent hooks](../guides/agent-hooks.md) |
| `devscout noop` | Exit 0, without doing anything |

The `parse` and `spans` diagnostics show the raw tree, both arms of every preprocessor group included.

## Global options

| Option | Effect |
| --- | --- |
| `-C <dir>` | Run as if `devscout` started in `<dir>` |
| `-h`, `--help` | Show the command list |
| `-V`, `--version` | Show the version |

## Help text

The whole help text, byte for byte, as `devscout help`, `devscout --help` and `devscout -h` print it:

```text
devscout -- fast code index for C# and TypeScript codebases.

usage: devscout <command> [args]

index
  init [scope ...]           register this repo, install hooks, first map
                             [--label L] [--no-hooks] [--no-map]
  map [scope ...] [--refresh] [--no-semantic]  build the index for the given scopes
  clear --older-than <days>  drop freshness rows older than N days
  clear --session <id>       drop one session's freshness rows

query
  find <query> [--resources] search the manifest by name or purpose
  refs <symbol>              references to a symbol   [--out --all --no-guess --no-dispatch --no-bus --pick N --json|--compact]
  read <symbol>              decl span + inbound refs [--no-guess --no-dispatch --no-bus --pick N --json|--compact]
  impact <file|symbol>       blast radius             [--hops N --no-guess --no-dispatch --no-bus --no-imports --pick N --json|--compact]
  tests <symbol>             tests reaching a symbol  [--no-guess --no-dispatch --no-bus --pick N --json|--compact]
  import-edges <file> --repo <id>  load a cross-repo edge export for `impact` to read
  compiler-facts run|import|status  optional engine-derived facts [--target --configuration --platform]
  export [--out FILE]        write this repository's whole fact set as NDJSON
  stats                      index + cache summary for this repo

plumbing
  parse <file.cs>            dump the parse tree
  spans <file.cs>            dump declaration spans
  extract-dump <file.cs>     dump extraction records
  audit --semantic <refs.jsonl>  score uses-member edges against a semantic oracle [--units F] [--defs F] [--json] [--assert F] [--fp-sites F]
  hook <read|bash>           agent hook filters, stdin -> stdout
  noop                       exit 0 (harness probe)

  -C <dir>                   run as if devscout started in <dir>
  -h, --help                 show this help
  -V, --version              show the version

docs: https://github.com/H3xas/devscout-rs/tree/v0.9.0/docs
```

## Flags by verb

### init

`init` has no usage line of its own; [Help text](#help-text) carries its spelling.

| Flag | Effect |
| --- | --- |
| `scope ...` | Directories to index, relative to the repository root; none means the whole root |
| `--label L` | A label stored with the root in the registry |
| `--no-hooks` | Skip the agent-hook install; see [Agent hooks](../guides/agent-hooks.md) |
| `--no-map` | Register the root without running the first map |

### map

`map` has no usage line of its own either; its spelling is in [Help text](#help-text).

| Flag | Effect |
| --- | --- |
| `scope ...` | Directories to index |
| `--refresh` | Accepted and ignored; every `map` is incremental |
| `--no-semantic` | Skip an admitted compiler-facts artifact and rebuild the syntax-only graph |

In a Git root, `map` also skips the untracked files and directories Git reports as ignored -- `.gitignore` at any depth, `.git/info/exclude`, `core.excludesFile`, with negations and tracked-file exceptions exactly as Git answers them -- so a local build into `build/`, `lib/` or `out/` cannot duplicate a source definition. A tracked file is always indexed, and a root with no `.git` entry is walked by the fixed directory-name list alone, with no `git` needed. In a Git root the ignore query is required: when `git` cannot be run, or its answer cannot be read, `map` exits 1 with `error: ignore query failed (…)` and leaves the previous index in place.

### clear

```text
usage: devscout clear --older-than <days>
```

```text
usage: devscout clear --session <id-or-prefix>
```

`--older-than <days>` drops freshness rows older than N days; `--session <id-or-prefix>` drops one session's rows, and an ambiguous prefix is a usage error.

### find

```text
usage: devscout find <query> [--resources]
```

| Flag | Effect |
| --- | --- |
| `--resources` | Include resource keys in the match list; by default they are reduced to a one-line trailer |

### refs, read, impact and tests

```text
usage: devscout refs <symbol> [--out] [--all] [--no-guess] [--no-dispatch] [--no-bus] [--pick N] [--json|--compact]
```

```text
usage: devscout read <symbol> [--no-guess] [--no-dispatch] [--no-bus] [--pick N] [--json|--compact]
```

```text
usage: devscout impact <file|symbol> [--hops N] [--no-iface] [--no-guess] [--no-dispatch] [--no-bus] [--no-imports] [--iface-max-fanin N] [--hub-max-indegree N] [--pick N] [--json|--compact]
```

```text
usage: devscout tests <symbol> [--no-guess] [--no-dispatch] [--no-bus] [--pick N] [--json|--compact]
```

| Flag | Verbs | Effect |
| --- | --- | --- |
| `--json` | all four | A versioned, machine-readable answer; see [JSON output](json-output.md) |
| `--compact` | all four | One line per hit, for piping; heuristic rows are marked `x` (`ext`) and `h` (`guess`) |
| `--pick N` | all four | Select the nth (one-based) candidate of an ambiguous member seed — a member seed with several declaring types; an out-of-range `N` is a usage error (exit code 2) |
| `--no-guess` | all four | Drop the `guess` tier, keeping `ext` |
| `--no-dispatch` | all four | Drop the `implements` and `overrides` edges that dependency-injection registrations record |
| `--no-bus` | all four | Skip `bus-hop` edges and restore the traversal and hub classification these verbs had before them |
| `--out` | `refs` | Also list outbound references |
| `--all` | `refs` | Lift the inbound and outbound row caps |
| `--hops N` | `impact` | How many hops from the seed to walk (at least 1) |
| `--no-iface` | `impact` | Do not widen through an interface to its implementations' callers |
| `--iface-max-fanin N` | `impact` | Treat an interface injected into more than N constructors as infrastructure and stop widening through it; `0` disables the brake |
| `--hub-max-indegree N` | `impact` | Stop widening through a file that N or more other files reference; `0` disables the in-degree half of the brake |
| `--no-imports` | `impact` | Ignore an imported cross-repo edge export |

The seed spellings these verbs accept are described in [Concepts](../concepts.md#seeds).

`tests` answers a symbol that no test file references with `no test references found`, and then a `hint (file name only, not a test reference):` block when a test file in the index is named after the symbol. Each hint row names the production symbols that file really references. A file name is not a test reference, so the block is a hint and never evidence, and it leaves every row, count and outcome untouched.

### import-edges

```text
usage: devscout import-edges <file> --repo <id>
```

`--repo` is required. See [Cross-repo impact](../guides/cross-repo-impact.md).

### compiler-facts

```text
usage: devscout compiler-facts <run|import|status>
```

```text
usage: devscout compiler-facts run --solution <path.sln|path.csproj> [--target T] [--configuration C] [--platform P] [--capabilities a,b] [--timeout-ms N] [--output-cap-bytes N]
```

```text
usage: devscout compiler-facts import <file> [--target T] [--configuration C] [--platform P]
```

```text
usage: devscout compiler-facts status
```

See [Compiler facts](../guides/compiler-facts.md) for what each sub-command admits and reports.

### export

```text
usage: devscout export [--out FILE]
```

Writes the header line, one line per fact and the trailer to stdout, or to `FILE` through a temporary file in the same directory and a rename. The stream is documented in [Fact export](fact-export.md).

| Flag | Effect |
| --- | --- |
| `--out FILE` | Write the stream to `FILE` instead of stdout. The directory must already exist; a failed run leaves no file at the path and keeps any file already there |

Any other argument — a positional, an unknown flag, `--out` with no value or with a value that is itself a flag, or `--out` given twice — is a usage error (exit 2) with the usage line on stderr and nothing on stdout.

### audit

```text
usage: devscout audit --semantic <refs.jsonl> [--units F] [--defs F] [--json] [--assert F] [--fp-sites F]
```

See [methodology](../benchmarks/methodology.md#resolver-precision). An `audit` argument error exits 1, not 2.

### hook

```text
usage: devscout hook <read|bash>
```

`hook` always exits 0: the filters answer on stdout and a refusal is a filter result, not a failed run. See [Agent hooks](../guides/agent-hooks.md).

### parse

```text
usage: devscout parse <file.cs>
```

### spans

```text
usage: devscout spans <file.cs>
```

### extract-dump

```text
usage: devscout extract-dump <file.cs>
```

## Output formats

`refs`, `read`, `impact`, and `tests` also take `--json` (machine-readable) or `--compact` (one line per hit, for piping). A zero-hit query is reported as a zero hit, not as an error — the tool never guesses a different symbol on your behalf, and an ambiguous name prints every candidate instead of picking one. See [JSON output](json-output.md) for the full contract.

A text or `--compact` answer to `refs`, `read`, `impact` and `tests` opens with the **full** count before any row cap applies: `N inbound (showing M)` on `refs` and `read`, `N files (showing M)` on `impact` and `tests`. A table that was truncated can therefore never be mistaken for the whole answer. On `refs`/`read`, N counts the five reference kinds and the four TypeScript kinds (`call`, `jsx-use`, `dispatch`, `import`) and excludes the `bus-hop` rows, which are possible routes rather than references; on `impact`, N is the number of files the walk reached, whether or not the row cap printed them; on `tests`, no row cap applies and the two numbers agree. A resolved answer that reached nothing states `0 inbound (showing 0)` or `0 files (showing 0)`. The line belongs to those four verbs and those two formats: `--json` carries the same numbers as its own table fields instead, `find` has neither the line nor a JSON shape, and an ambiguous, not-found or fallback answer states no count at all.

A text or `--compact` answer to `refs`, `read`, `tests` and `impact`, and a text answer to `find` — which has no format flag — ends with a `next:` line naming the cheapest verb to run next:

```text
next: devscout tests Widgetworks.Catalog.InMemoryCatalogStore | devscout impact InMemoryCatalogStore.cs
```

The line is written to be pasted, so an argument a shell would read as its own — a space, `#`, `$`, a quote, a glob — is single-quoted. Every suggestion was resolved against the verb it names before it was printed, so each command on the line runs as it stands. An answer with nothing left to suggest omits the line entirely. Under `--json` the same suggestions arrive as a `next` array of argv arrays, without the binary name; see [JSON output](json-output.md#next).

## Exit codes

Every status a verb returns is one of four named constants; [Exit codes](exit-codes.md) lists them, what produces each, and the statuses of the plumbing verbs that differ from the query verbs' convention. On `refs`, `read`, `impact` and `tests` the exit code follows the answer's outcome, as [JSON output](json-output.md#outcome) describes.

## read

`devscout read <symbol>` returns the indexed declaration's start and end lines, the verbatim source in that span, and its inbound references. `<symbol>` is a type name or a member seed (see [Seeds](../concepts.md#seeds) — a bare name, `Type.Member`, or `Namespace.Type.Member`); a member seed answers with its own declaration line and inbound references, carrying no span (nothing records an end line for a member on its own). Use `--compact` for a line-oriented summary or `--json` for structured output. References that originate inside the target declaration itself are excluded from inbound rows and counts, so recursive and other self-references do not look like external callers.
