# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`uses-member` edges carry `tier` and `member`.** `tier` names which heuristic tier
  emitted the edge (`ext` for extension-method lookup, `guess` for the scored name match);
  `member` names the member the reference reads or calls, on precise and heuristic edges
  alike. Only `tier` (and `heuristic`) are omitted on a precise edge; `member` is written
  there too, which is why a precise edge's bytes change under this schema bump as well.
- **`--no-guess` on `refs`, `read`, `impact`, and `tests`.** Admits only the `ext` tier's
  edges into the query surface, dropping every scored-tier guess; compact output marks the
  two tiers `x` and `h`.
- **`stats.heuristic_by_tier` splits `heuristic_edge_count` by tier.** Always written, with
  `ext` and `guess` counts that sum to `heuristic_edge_count`.
- **A lightweight `.csproj` project model.** Hand-scans `ProjectReference`,
  `Microsoft.NET.Test.Sdk`, and `IsTestProject` out of `.csproj` and `Directory.Build.props`
  files, with no MSBuild evaluation. Discovered projects are persisted as `units` in
  `graph.json` and mirrored in a `project-units.json` sidecar; editing a `.csproj` is now
  itself a rebuild trigger.

### Changed

- **Graph schema bumped to 2.** `tier`, `member`, and `units` are the new keys; a v1
  graph.json is rebuilt automatically on the next `map` rather than read as-is.
- **`global using` scopes to the project that declared it when a project model exists.**
  Without a model every `global using` is still repo-wide, unchanged.
- **Tier (f) admits an extension class from an enclosing namespace, not just an imported
  one.** `App.Ext` is now visible from `App.Ext.Deep` with no `using` at all, matching C#'s
  own namespace-visibility rule.

### Fixed

- **A call no longer vouches through a property or field.** `entity.Property(x => x.Id)` has
  no overload-resolution path to a property or field of that name, so the scored tier no
  longer lets one stand in as evidence for a call shape it cannot answer.
- **A scored guess under an external receiver must be nominally assignable to it.** A
  candidate the receiver's type could never actually be is now a disproved guess rather than
  a weak one.
- **A scored guess never crosses into a project the reference site cannot reach, or into a
  test project from non-test code.** When a project model exists, both are now structural
  refusals rather than name-only guesses.

### Benchmarks

- **Resolver-precision benchmark against a compiler oracle.** `tools/scout-semantic` (a C# console
  project, not part of the crate build) emits one record per member reference from a compiled
  solution; the plumbing verb `devscout audit --semantic <refs.jsonl>` scores `uses-member`
  edges per tier for precision, recall, external-receiver leaks and cross-project impossibility,
  with `--assert` thresholds for CI. `fixtures/csharp-semantic` pins the defect shapes; baseline
  figures on the pinned MassTransit corpus are in `docs/benchmarks/results/2026-09-resolver-precision.md`.

## [0.3.0] - 2026-08-27

### Added

- **`read` verb.** Serves a symbol's declaration span plus its inbound callers — with real
  declaration spans for TypeScript as well as C#. The first read of an indexed file offers
  the nearest symbol, including ranged reads, which offer the symbol nearest the requested
  range. References originating inside the target's own declaration span are excluded from
  inbound results and counts.
- **`find` ranking.** Results are ranked by precise inbound reference counts. References
  originating in the same file are excluded from a file's inbound count.

### Changed

- **`init` language census reports three honest tiers**: C# fully supported;
  TS/TSX/JS indexed and graphed with narrower edge coverage; other counted extensions
  present, not indexed. The census no longer understates TypeScript.
- **The content-database default resolves at runtime** from the home directory, matching
  the registry path's resolution, instead of a compile-time manifest path. The
  `SCOUT_CONTENT_DB` override is unchanged.
- Crate-wide rustfmt adoption and a full public-API rustdoc pass
  (`missing_docs = "warn"` at zero warnings). CI enforces formatting from this release on.

### Fixed

- **Bare names no longer bind to nested types the reference site cannot name.** A bare,
  undotted type name reaches a nested type only when the reference site sits inside the
  nesting chain or inherits the enclosing type; other bare references fall through as
  unresolved or external instead of emitting a false precise edge that `impact` then
  widens through (for example, a bare `Claim` under `using System.Security.Claims`
  binding to an unrelated nested test class).
- **Generic arity is matched exactly during resolution.** A type reference resolves only
  to definitions with a matching type-parameter count; arity-overloaded siblings stay
  distinct, and mismatched references fall through as unresolved or external instead of
  binding a wrong same-named definition. The fragment cache generation is bumped, so the
  first run after upgrading re-extracts.

### Benchmarks

- The published scorecard was measured on 0.2.0 and is not re-measured in this release.
  `find` output ordering and reference resolution changed in 0.3.0, so those numbers
  describe 0.2.0 until the next benchmark round.

## [0.2.0] - 2026-08-25

Both entries are fixes for defects the first benchmark round found in this tool and
recorded against itself (`docs/benchmarks/results/2026-08.md`, "Defects this run found
in its own method"). Both change observable output, hence the minor bump.

### Changed

- **`find`: every manifest-pool row now carries a line.** Rows were `path: purpose`,
  with no line, so a follow-up could not open the hit at a position and the row failed
  the benchmark's follow-up-reach rule. They are now `path:line: purpose`, where `line`
  is that file's own first declaration in the name index, falling back to line 1 for a
  file the index carries no declared symbol for at all. The declaration block above it
  is unchanged.

### Fixed

- **`refs --all` now lifts the inbound cap too, not just the outbound one.** `--all`
  lifted `OUTBOUND_CAP` only, so a `refs` answer truncated at `INBOUND_CAP = 30` had no
  flag that could recover it and silently returned fewer referring files than the graph
  held. `--all` now lifts both caps. Output without `--all` is unchanged.

## [0.1.0]

Initial public release: `map`, `find`, `refs`, `impact`, `init`, `stats`, `clear` over a
C# and TypeScript/JavaScript index.
