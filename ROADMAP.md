# Roadmap

Direction, not commitment: items ship when they are ready. Details and discussion live in
the linked issues; anything not listed here is fair game for a proposal.

## Near term

- **Reproducible agent-lane benchmark harness, then a 0.3.0 benchmark round.** The
  tool-call proxy table in the benchmark docs currently lacks an in-repository
  reproduction path, and the published scorecard was measured on 0.2.0; the corpus pin,
  lane harness, commands, and fresh numbers land together. (A different benchmark family
  already has this: [`bench/semantic.sh`](bench/semantic.sh) is a one-command, in-repo
  reproduction path for the resolver-precision numbers — pinned fixture, one script, one
  command. The agent-lane harness this item is about is still open.)

## TypeScript / JavaScript semantic coverage

TS/TSX/JS files are parsed with tree-sitter and resolved into the graph with a narrower set
of edge kinds than C# (see README → Limitations). Planned as three independent stages:

1. **Resolution wins on the existing stack** — tsconfig path aliases, barrel-file
   (`index.ts` re-export) following, and JSX component-usage edges, all derived from the
   AST already parsed today. No new dependencies. Aliases from the repo-root tsconfig chain,
   one barrel hop, and `jsx-use` edges shipped in 0.2.0; nearest-`tsconfig.json` alias
   scoping and chained barrels (up to eight hops, cycle-guarded) are in Unreleased. Still
   open in this stage: `export * as ns from` namespace re-exports, and bare specifiers that
   name a workspace package by its `package.json` name.
2. **Semantic binding via oxc** — adopt `oxc_parser`/`oxc_semantic`/`oxc_resolver` to
   replace name-based reference matching with real scope and symbol binding, and module
   resolution that understands the TypeScript config.
3. **Optional type-aware sidecar** — chained-call and `.d.ts`-level resolution requires the
   TypeScript checker; an optional external collector could feed those edges into the index.
   Considered only after stages 1–2 are exhausted.

## C# resolution depth

The C# side carries the same open gap as TS at the top end: no resolution through chained
method calls, and no analysis inside external package internals. Revisited after the TS
stages prove out the approach.

The compiler-backed enrichment layer — an optional, cached input to `resolve` produced by
the Roslyn oracle in `tools/scout-semantic` out of process, never on the hook path — is
designed in [`docs/design/compiler-enrichment.md`](docs/design/compiler-enrichment.md),
with the shipping gate it must clear written down before any implementation.
