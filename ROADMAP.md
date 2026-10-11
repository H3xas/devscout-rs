# Roadmap

Direction, not commitment: items ship when they are ready. Details and discussion live in
the linked issues; anything not listed here is fair game for a proposal.

Reviewed for 0.9.0

## Near term

- **A reproducible agent-lane benchmark harness.** The protocol for measuring what an agent
  does with the index is written down, but no lane has been run against the pinned public
  corpora, and the agentic row on the scorecard is still a preliminary round measured on an
  older build. The corpus pin, a one-command in-repo reproduction path beside
  [`bench/semantic.sh`](bench/semantic.sh), and fresh figures land together —
  [#40](https://github.com/H3xas/devscout-rs/issues/40).
- **A deny-warnings `cargo clippy` gate in CI.** CI builds, tests and runs clippy, but without
  `-D warnings`; a local `cargo clippy --all-targets` run still reports a pre-existing baseline
  of style-level findings. Rather than silencing them piecemeal, the baseline closes to zero
  first and the gate lands already green —
  [#41](https://github.com/H3xas/devscout-rs/issues/41).

## TypeScript / JavaScript semantic coverage

TS/TSX/JS files are parsed with tree-sitter and resolved into the graph with a narrower set
of edge kinds than C# (see README → Language support). Three independent stages are open:

1. **The remaining single-file resolution cases** — namespace re-exports
   (`export * as ns from`) contribute their import edge but no name mapping, and a bare
   specifier that names a workspace package by its `package.json` name is not resolved to
   that package. Both are wins on the AST already parsed today, with no new dependency —
   [#42](https://github.com/H3xas/devscout-rs/issues/42).
2. **Semantic binding via oxc** — adopt `oxc_parser`/`oxc_semantic`/`oxc_resolver` to replace
   name-based reference matching with real scope and symbol binding, and module resolution
   that understands the TypeScript config —
   [#43](https://github.com/H3xas/devscout-rs/issues/43).
3. **An optional type-aware sidecar** — chained-call and `.d.ts`-level resolution requires the
   TypeScript checker; an optional external collector could feed those edges into the index.
   Considered only after stages 1–2 are exhausted —
   [#44](https://github.com/H3xas/devscout-rs/issues/44).

## C# resolution depth

The C# side carries the same open gap as TS at the top end: no resolution through chained
method calls, and no analysis inside external package internals. Revisited after the TS
stages prove out the approach —
[#45](https://github.com/H3xas/devscout-rs/issues/45).
