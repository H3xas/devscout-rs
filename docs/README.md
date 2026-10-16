# devscout documentation

Plain Markdown, versioned with the code: the copy under a release tag describes that release.

## I want to…

| I want to | Read |
| --- | --- |
| Install devscout or verify a downloaded binary | [Install](install.md) |
| Understand the graph, edges, tiers, seeds and outcomes | [Concepts](concepts.md) |
| Know what devscout does not do | [Limitations](limitations.md) |
| Look up a command or flag | [CLI reference](reference/cli.md) |
| Parse `--json` output | [JSON output](reference/json-output.md) |
| Read a whole repository's facts in one pass | [Fact export](reference/fact-export.md) |
| Find where the index is stored | [Storage](reference/storage.md) |
| Set an environment variable | [Environment variables](reference/environment.md) |
| Install or remove the agent hooks | [Agent hooks](guides/agent-hooks.md) |
| Enrich C# with compiler-verified facts | [Compiler facts](guides/compiler-facts.md) |
| Connect impact across repositories | [Cross-repo impact](guides/cross-repo-impact.md) |
| Upgrade from an earlier version | [Upgrading](guides/upgrading.md) |
| Fix a stale, empty or ambiguous answer | [Troubleshooting](guides/troubleshooting.md) |
| Learn why a reference resolved the way it did | [How names resolve](explanation/resolution.md) |
| See the evidence behind accuracy and cost claims | [Benchmarks](benchmarks/README.md) |

## How-to guides

- [Agent hooks](guides/agent-hooks.md): what `init` installs, what the hooks do, and how to remove
  them.
- [Compiler facts](guides/compiler-facts.md): acquire, import and read an optional
  compiler-derived fact artifact.
- [Cross-repo impact](guides/cross-repo-impact.md): load an edge export so `impact` reaches other
  repositories.
- [Upgrading](guides/upgrading.md): artifact layout changes, graph schema history, moved pages.
- [Troubleshooting](guides/troubleshooting.md): stale, empty and ambiguous answers.

## Reference

- [CLI](reference/cli.md): every command and flag.
- [JSON output](reference/json-output.md): the `--json` answer contract.
- [Fact export](reference/fact-export.md): the versioned NDJSON stream `export` writes, its
  schema and its refusals.
- [Storage](reference/storage.md): where artifacts and stores live.
- [Environment variables](reference/environment.md).
- [C# constructs](reference/csharp-constructs.md): which C# constructs produce facts.
- [Rust constructs](reference/rust-constructs.md): which Rust items and paths are indexed and
  bound.
- [.NET targets](reference/dotnet-targets.md): which targets, project systems and framework
  profiles the compiler-fact path is qualified on.
- [Call-site evidence](reference/call-site-evidence.md): what each surface records about a call
  site.
- [Install](install.md): crates.io, release binaries, building from source, verifying a download.

## Explanation

- [Concepts](concepts.md), with a [glossary](concepts.md#glossary).
- [How names resolve](explanation/resolution.md): tiers, receiver typing, binding rules,
  TypeScript imports and Rust paths.
- [Limitations](limitations.md): one line per known limit, each linked to its detail.

## Benchmarks

[Benchmarks](benchmarks/README.md) hold every claim about speed, cost or accuracy, with the
command that produced it, the scorecard, and the dated results.

## Design records

Historical records, kept as written:

- [Compiler-backed enrichment layer](design/compiler-enrichment.md).

## Contributing

[CONTRIBUTING.md](../CONTRIBUTING.md) covers building, testing and submitting a change;
[ARCHITECTURE.md](../ARCHITECTURE.md) maps each module and says where a change goes.
