# Roadmap

Direction, not commitment: items ship when they are ready. Details and discussion live in
the linked issues; anything not listed here is fair game for a proposal.

## Near term

- **Reproducible agent-lane benchmark harness, then a 0.3.0 benchmark round.** The
  tool-call proxy table in the benchmark docs currently lacks an in-repository
  reproduction path, and the published scorecard was measured on 0.2.0; the corpus pin,
  lane harness, commands, and fresh numbers land together.

## TypeScript / JavaScript semantic coverage

TS/TSX/JS files are parsed with tree-sitter and resolved into the graph with a narrower set
of edge kinds than C# (see README → Limitations). Planned as three independent stages:

1. **Resolution wins on the existing stack** — tsconfig path aliases, barrel-file
   (`index.ts` re-export) following, and JSX component-usage edges, all derived from the
   AST already parsed today. No new dependencies.
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
