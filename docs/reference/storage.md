# Storage

Hand-written for now; this page will be generated from the artifact definitions.

## Where it stores things

Inside a git repository, artifacts live under the git common directory, so they never show up as
untracked files and are shared correctly by worktrees:

```text
<git-common-dir>/scout/manifest.json              file -> purpose + symbol index
<git-common-dir>/scout/index-state.json           HEAD + timestamp the index was built at
<git-common-dir>/scout/graph/graph.json           definitions, edges, and project units
<git-common-dir>/scout/graph/fragments-v<N>.json  per-file extraction cache (incremental map)
<git-common-dir>/scout/graph/project-units.json   csproj staleness sidecar (present only with a project model)
<git-common-dir>/scout/graph/rust-crates.json     Cargo crate-model staleness sidecar (present only with a Cargo package in scope)
<git-common-dir>/scout/graph/compiler-facts-v<N>.json  optional versioned compiler-fact artifact; absent unless `compiler-facts run|import` has admitted one
<git-common-dir>/scout/log/queries.jsonl          query-verb telemetry, one JSON line per answered invocation (opt-in; SCOUT_TELEMETRY=1)
```

`<N>` is the artifact's generation, which changes when the artifact's shape changes. The
[upgrading guide](../guides/upgrading.md) covers what to do when a release changes the layout.

Outside a git repository the same tree is written to `<root>/.scout/` instead. `devscout init`
also adds `.scout` to the repository's local exclude file so the legacy location cannot be
committed by accident.

The compiler-fact artifact is described in [Compiler facts](../guides/compiler-facts.md), and
the imported cross-repo edge set that `import-edges` writes beside `graph.json` in
[Cross-repo impact](../guides/cross-repo-impact.md).

## Stores outside the repository

Two stores live outside the repo:

- **Registry** — `$HOME/.claude/scout/repos.json`, the list of roots `devscout` knows about.
- **Read-freshness cache** — `<root>/.scout/cache.db` plus a content-addressed `content.db`,
  both SQLite. These are only written by the agent hooks (`devscout hook read|bash`); plain CLI
  use does not touch them.

[Environment variables](environment.md) move the registry and the content database.

## Ordering

Artifact ordering is fixed and deterministic by design; do not rely on it staying
byte-identical across minor versions. Ordering is load-bearing but not a stability promise.
