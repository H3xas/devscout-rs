# .NET target qualification -- held-out run (2026-09-17)

**Superseded by
[2026-09-21-dotnet-target-held-out.md](2026-09-21-dotnet-target-held-out.md).** This run's own
`expected-held-out.json` already registered an `unsupported_coverage` minimum per stratum, but
this run's script never computed that metric, so the `Threshold check` table below checked only
precision and recall and its `meets threshold` verdicts did not cover every registered minimum.
The 2026-09-21 run, at the same corpus and SHA, computes `unsupported_coverage` and reports
`MISS` on all three measured strata against that same registered floor. Kept here as the dated
historical record; do not read its `meets threshold` verdicts as current.

Single-run banner: one repetition per stratum, per
[docs/benchmarks/README.md](../../../docs/benchmarks/README.md)'s honesty statement.
Predictions registered before this run in
[expected-held-out.json](../../../fixtures/csharp-target-qualification/expected-held-out.json).

Corpus: `serilog/serilog` at the SHA pinned in
[bench/corpus.lock](../../../bench/corpus.lock) (`csharp-target-held-out`).

| Stratum (TFM) | Restore | Build | Precision | Recall (all) | Recall (precise) |
| --- | --- | --- | --- | --- | --- |
| net8.0 | ok | ok | 0.925 | 0.797 | 0.76 |
| net6.0 | ok | ok | 0.925 | 0.797 | 0.76 |
| netstandard2.0 | ok | ok | 0.944 | 0.904 | 0.863 |

## Threshold check

Registered minimums from `expected-held-out.json`, checked against the row above.

| Stratum (TFM) | Precision >= min | Recall (all) >= min | Result |
| --- | --- | --- | --- |
| net8.0 (modern) | 0.925 >= 0.9: True | 0.797 >= 0.6: True | meets threshold |
| net6.0 (modern) | 0.925 >= 0.9: True | 0.797 >= 0.6: True | meets threshold |
| netstandard2.0 (netstandard) | 0.944 >= 0.9: True | 0.904 >= 0.5: True | meets threshold |

## Where peers win

Not measured in this run: a peer/ripgrep baseline comparison. This run scores
devscout's own compiler-fact edges against the pinned oracle only.

## Not measured

`net471`, `net462`: Windows-conditional TFMs in this corpus's own `.csproj`,
out of scope for this non-Windows run. Recorded here rather than silently skipped.
