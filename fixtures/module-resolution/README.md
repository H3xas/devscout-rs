# Module resolution cases

Each subdirectory is a separate repository root. `expected.tsv` records TypeScript 5.9.3 `resolveModuleName` targets with declaration files excluded. The unresolved reason is the resolver's stable explanation for a target TypeScript did not find.

Run `node bench/ts_resolution_truth.cjs <path-to-typescript.js> <case-directory>` from the repository root to inspect TypeScript's target for one case. The cases pin relative paths and extension rules in node10, node16, nodenext and bundler modes, plus a named npm workspace member.
