# .NET target qualification -- held-out run (2026-09-21)

Single-run banner: one repetition per stratum, per
[docs/benchmarks/README.md](../../../docs/benchmarks/README.md)'s honesty statement.
Predictions registered before this run in
[expected-held-out.json](../../../fixtures/csharp-target-qualification/expected-held-out.json).
Precision/recall uncertainty below is a single-run Wilson 95% interval computed from
each row's own tp/fp/denominator counts, not from repeated runs.

Corpus: `serilog/serilog` at the SHA pinned in
[bench/corpus.lock](../../../bench/corpus.lock) (`csharp-target-held-out`).

Corpus's own declared TargetFrameworks on this (non-Windows) worker: `net8.0 net6.0 netstandard2.0`.

| TFM | Measured | Restore | Build | Oracle/audit | Precision (95% CI) | Recall all (95% CI) | Recall precise | Denominator | Unsupported coverage |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| net5.0 | no | -- | -- | -- | n/a | n/a | n/a | n/a | n/a |
| net6.0 | yes | ok | ok | ok | 0.925 [0.902, 0.943] | 0.797 [0.767, 0.824] | 0.76 | 778 | 0.881 |
| net7.0 | no | -- | -- | -- | n/a | n/a | n/a | n/a | n/a |
| net8.0 | yes | ok | ok | ok | 0.925 [0.902, 0.943] | 0.797 [0.767, 0.824] | 0.76 | 778 | 0.881 |
| net9.0 | no | -- | -- | -- | n/a | n/a | n/a | n/a | n/a |
| netstandard2.0 | yes | ok | ok | ok | 0.944 [0.923, 0.959] | 0.904 [0.880, 0.924] | 0.863 | 699 | 0.86 |
| netstandard2.1 | no | -- | -- | -- | n/a | n/a | n/a | n/a | n/a |
| netcoreapp3.1 | no | -- | -- | -- | n/a | n/a | n/a | n/a | n/a |
| net40 | no | -- | -- | -- | n/a | n/a | n/a | n/a | n/a |
| net472 | no | -- | -- | -- | n/a | n/a | n/a | n/a | n/a |
| net48 | no | -- | -- | -- | n/a | n/a | n/a | n/a | n/a |

## Threshold check

Registered minimums from `expected-held-out.json`, checked per registered stratum. A
stratum with no declared TFM on this worker is reported `not exercised`, never omitted.

| Stratum | TFMs measured | Precision >= min | Recall >= min | Unsupported coverage >= min | Result |
| --- | --- | --- | --- | --- | --- |
| modern | net6.0 | 0.925 >= 0.9: True | 0.797 >= 0.6: True | 0.881 >= 1.0: False | MISS |
| modern | net8.0 | 0.925 >= 0.9: True | 0.797 >= 0.6: True | 0.881 >= 1.0: False | MISS |
| netstandard | netstandard2.0 | 0.944 >= 0.9: True | 0.904 >= 0.5: True | 0.860 >= 1.0: False | MISS |
| netcoreapp3.1 | none | n/a | n/a | n/a | not exercised: none of `netcoreapp3.1` is declared by the corpus on this worker |
| framework-f1 | none | n/a | n/a | n/a | not exercised: none of `net40`, `net472`, `net48` is declared by the corpus on this worker |

## Where peers win

Not measured in this run: a peer/ripgrep baseline comparison. This run scores
devscout's own compiler-fact edges against the pinned oracle only.

## Not measured

TFMs registered in a stratum but not declared by this corpus on this (non-Windows)
worker, recorded here rather than silently skipped:

- `net5.0`: not declared by the corpus's own TargetFrameworks on this worker (observed: `net8.0 net6.0 netstandard2.0`).
- `net7.0`: not declared by the corpus's own TargetFrameworks on this worker (observed: `net8.0 net6.0 netstandard2.0`).
- `net9.0`: not declared by the corpus's own TargetFrameworks on this worker (observed: `net8.0 net6.0 netstandard2.0`).
- `netstandard2.1`: not declared by the corpus's own TargetFrameworks on this worker (observed: `net8.0 net6.0 netstandard2.0`).
- `netcoreapp3.1`: not declared by the corpus's own TargetFrameworks on this worker (observed: `net8.0 net6.0 netstandard2.0`).
- `net40`: not declared by the corpus's own TargetFrameworks on this worker (observed: `net8.0 net6.0 netstandard2.0`).
- `net472`: not declared by the corpus's own TargetFrameworks on this worker (observed: `net8.0 net6.0 netstandard2.0`).
- `net48`: not declared by the corpus's own TargetFrameworks on this worker (observed: `net8.0 net6.0 netstandard2.0`).
