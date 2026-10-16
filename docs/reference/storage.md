# Storage

Hand-written for now; this page will be generated from the artifact definitions.

## Where it stores things

Inside a git repository, artifacts live under the git common directory, so they never show up as
untracked files. The index belongs to one worktree at a time -- the worktree that last ran
`map` there, named by its Git administrative directory:

```text
<git-common-dir>/scout/manifest.json              file -> purpose + symbol index
<git-common-dir>/scout/index-state.json           the freshness record: owner, generation, indexed HEAD,
                                                  content digests, and the identities of the two artifacts
<git-common-dir>/scout/index-digests.json         per-file content digest for every indexed file, for this generation
<git-common-dir>/scout/map.lock                   the lock a `map` holds while it publishes
<git-common-dir>/scout/graph/graph.json           definitions, edges, and project units
<git-common-dir>/scout/graph/fragments-v<N>.json  per-file extraction cache (incremental map), stamped with its owning worktree
<git-common-dir>/scout/graph/project-units.json   csproj staleness sidecar (present only with a project model)
<git-common-dir>/scout/graph/rust-crates.json     Cargo crate-model staleness sidecar (present only with a Cargo package in scope)
<git-common-dir>/scout/graph/compiler-facts-v<N>.json  optional versioned compiler-fact artifact; absent unless `compiler-facts run|import` has admitted one
<git-common-dir>/scout/log/queries.jsonl          query-verb telemetry, one JSON line per answered invocation (opt-in; SCOUT_TELEMETRY=1)
```

`<N>` is the artifact's generation, which changes when the artifact's shape changes. The
[upgrading guide](../guides/upgrading.md) covers what to do when a release changes the layout.

## One owner per index

The paths above are shared by every linked worktree of a repository, but the index is not. A
`map` records the worktree that produced it, and a query asked from any other worktree reports
`unknown` with reason `other-worktree` rather than an answer labelled `fresh`. The reading
worktree runs `map` once to take the index over. A `map` in one worktree also reuses no extracted
fragment another worktree wrote: the fragment cache carries its owner too.

The freshness record is written by atomic rename and before that is preceded by an in-progress
marker, so a query racing a `map` -- or reading after a crash -- sees the previous generation
intact or a run in flight, never a half-written pair. A `map` holds `map.lock` for the whole run,
so two maps of one repository serialize instead of interleaving. The record is local to the
repository; nothing about it is transmitted anywhere.

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

## What is a contract and what is not

None of the files above is a contract. They are this tool's own storage: their
shape, their key order and even their names change between releases, and a
consumer that reads them is reading an implementation detail.

[`devscout export`](fact-export.md) is the contract. It reads these artifacts
and writes one versioned, schema-described stream, so a storage change stays an
internal change rather than becoming a release every consumer has to follow. A
fact that is not in that stream is not published, whatever the private files
happen to contain.
