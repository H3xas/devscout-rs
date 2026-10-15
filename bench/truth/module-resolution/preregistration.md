# TypeScript module resolution comparison

## Registered inputs

The public fixtures, exact commits, licenses, directory prefixes, modes and minimum truth-resolvable counts are in `fixtures.json`. `bench/corpus.lock` pins the same commits. The selected prefixes each have at least 100 truth-resolvable imports in the preflight and at least 20 individually. Synthetic repositories under `fixtures/module-resolution/` cover the negative cases and manifest shapes absent from a qualifying public fixture.

The implementation base is `f136d8ba30ad5f4d6f80f2c5c60173349abd599c`. The two arms are `devscout-base` built from that commit and `devscout-head` built from the committed head under review. Each runs once on its own pristine copy of every corpus. A code fix creates a new head arm run with its own stored outputs; the registered inputs stay fixed.

## Truth

Use TypeScript 5.9.3 from the local offline npm cache. `bench/ts_resolution_truth.cjs` parses the nearest declaring tsconfig chain for each importer, sets `noDtsResolution: true`, determines import or require usage, and calls `resolveModuleName` for every literal import, re-export, import-equals, `require` and dynamic `import()` site. It overlays workspace members under virtual `node_modules` paths and mapped build outputs without installing dependencies. An implementation target is a repository-relative path only if it is among the indexed TypeScript and JavaScript implementation files. A declaration, external package or unresolved specifier has an empty target. Non-literal dynamic imports are outside this scoring set and have their own graph count.

`expected.tsv` under the synthetic fixtures holds the TypeScript target or one of the resolver's closed unresolved reasons for each recorded site. The reason annotations are checked by the end-to-end fixture test. Refused configuration is checked separately: TypeScript may tolerate a shape the declared resolver intentionally refuses.

## Scoring and gate

Each site is keyed by importing file, one-based line and specifier. A direct `import` edge with no `via` is compared with truth. `agree` means the edge names the truth file. `agree-unresolved` means truth names no indexed implementation file and there is no edge. `miss` means truth names a file and no edge exists. `wrong-target` means an edge names another file. `spurious` means an edge exists where truth names no indexed implementation file. Every disagreement is listed in the results file with its class, truth and actual target.

For each mode pooled across its registered public fixtures, agreement is `agree / (agree + miss + wrong-target)` and must be at least 0.95. `wrong-target` and `spurious` must each be zero. Every synthetic positive and negative row must match, including its unresolved reason. Base and head scores are stored separately and rendered together by `bench/ts_resolution_report.py` into a dated results file. The registration manifest hashes the truth script, scorer, renderer, runner, fixture list, synthetic expectations, this procedure and corpus pins; verification precedes each measured run and score.
