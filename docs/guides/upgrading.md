# Upgrading

[CHANGELOG.md](../../CHANGELOG.md) lists what each release changes. This guide covers the steps an
upgrade can need.

## Artifact layout before 1.0

While the version is `0.x`, minor bumps may change artifact layout — delete the artifact directory
and re-run `devscout map` after upgrading. Where the artifact directory is depends on whether the
root is a git repository; see [Storage](../reference/storage.md).

The per-file extraction cache and the compiler-fact artifact carry a generation in their file
names (`fragments-v<N>.json`, `compiler-facts-v<N>.json`). After deleting the artifact directory,
an optional compiler-fact artifact has to be acquired or imported again, as the
[compiler facts guide](compiler-facts.md) describes.

## Graph schema history

- **Graph schema 2 adds `member`, `tier`, `units`, and `stats.heuristic_by_tier`.** `member` is
  written on every `uses-member` edge, `tier` on the heuristic ones; `units` (the discovered
  `.csproj` projects) is appended last. A `uses-member` edge also carries an optional `source` key
  (`"semantic"` or `"semantic-discovered"`), set only when an admitted compiler-facts artifact
  produced the edge and omitted entirely otherwise, so a syntax-only graph stays byte-identical to
  one built before compiler-fact consumption existed. A v1 graph.json is rebuilt automatically on
  the next `map`.
- **Schema 3 adds the `implements`/`overrides` edge kinds** and their `edges_by_kind` counters; a
  v2 graph.json is rebuilt the same way a v1 one is.

The `--json` answers carry their own `schema_version`, described in
[JSON output](../reference/json-output.md#schema_version).

## Moved documentation pages

The JSON contract, the C# construct catalogue, the .NET target catalogue and the call-site
evidence page moved under [`docs/reference/`](../README.md#reference) as
[JSON output](../reference/json-output.md), [C# constructs](../reference/csharp-constructs.md),
[.NET targets](../reference/dotnet-targets.md) and
[Call-site evidence](../reference/call-site-evidence.md). Each old path answers with a one-line
pointer for one release and is removed in the next minor release;
[CHANGELOG.md](../../CHANGELOG.md) names the old paths.
