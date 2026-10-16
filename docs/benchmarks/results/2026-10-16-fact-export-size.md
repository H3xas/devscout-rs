# Fact export size receipt

Run date: 2026-10-16. Head: `f417f5d1b24aaca888685d40ed005c68e15abfee`.

What one `devscout export` costs on the two pinned corpora, beside the index it
reads and one `refs` query's cost on the same index. This is a measurement, not
a threshold: nothing here fails a build, and no target is set from it.

Machine: Apple M2 Max, 12 cores, 64 GB, macOS 26.2, arm64. Release build
(`cargo build --release --bins`), binary reporting version `0.9.0`. Both corpora
were cloned at their pinned commits and mapped once by this same build; every
run below follows that map without changing the tree.

| Corpus | Pin | Files | Definitions | Edges |
| --- | --- | ---: | ---: | ---: |
| MassTransit | `MassTransit/MassTransit` @ `855cf1752c94ca9498e0c45ce8d09fdc9e957dd6` | 5634 | 9951 | 159755 |
| Nx | `nrwl/nx` @ `1ce5e91f5ec13cf8812f3e67e1fae33079336876` | 6674 | 8397 | 59856 |

## Index sizes on disk

| Corpus | `graph.json` | Fragment cache | `manifest.json` | Digest table | Total |
| --- | ---: | ---: | ---: | ---: | ---: |
| MassTransit | 52.8 MB | 63.6 MB | 1.41 MB | 0.81 MB | 118.6 MB |
| Nx | 17.2 MB | 6.8 MB | 1.22 MB | 0.81 MB | 26.1 MB |

## The export

`devscout export > /dev/null`, three runs, `/usr/bin/time -l`.

| Corpus | Bytes | Wall time (median of 3) | Peak RSS (max of 3) | Export ÷ index |
| --- | ---: | ---: | ---: | ---: |
| MassTransit | 55,935,641 (53.3 MB) | 0.60 s | 402.3 MB | 0.45× |
| Nx | 20,044,525 (19.1 MB) | 0.30 s | 134.2 MB | 0.73× |

Per-run detail: MassTransit 0.58 / 0.60 / 0.61 s and 401.0 / 402.3 / 402.2 MB;
Nx 0.30 / 0.30 / 0.31 s and 134.2 / 132.5 / 134.0 MB.

## The `refs` query on the same index

`devscout refs <seed> --json`, one run each, `/usr/bin/time -l`, both answers
resolved rather than ambiguous or empty.

| Corpus | Seed | Peak RSS | Wall time |
| --- | --- | ---: | ---: |
| MassTransit | `IBus` | 165.7 MB | 0.27 s |
| Nx | `createProjectGraphAsync` | 65.2 MB | 0.19 s |

## What the export carries

The trailer's per-family counts on each corpus:

| Family | MassTransit | Nx |
| --- | ---: | ---: |
| `definition` | 9951 | 8397 |
| `edge` | 159755 | 59856 |
| `publish-site` | 10668 | 5 |
| `handler-registration` | 1457 | 0 |
| `di-registration` | 143 | 0 |
| `unresolved-import` | 0 | 4766 |
| `test-file` | 1088 | 1024 |

Two of those rows are worth reading beside the coverage table: the Nx corpus
records 5 publish sites and no handler or dependency registrations because those
families are C#-only in this release, and MassTransit records no unresolved
import because that family is TypeScript-only. Coverage in each export's header
says which is which.

## Reading the numbers

- Peak RSS on both corpora is dominated by inputs rather than by output: the
  graph and the fragment cache are both held while the facts are built, and the
  whole stream is rendered before a byte is written, which is what makes a
  refusal total.
- The export is smaller than the index it reads on both corpora (0.45× and
  0.73×). The index carries resolution inputs the export does not publish —
  member tables, base lists, per-file extraction records — and the export's
  edges carry no key the index already dropped before writing `graph.json`.
- The `refs` figures are one query each, not a comparable workload: they are
  here because the plan asked for the export's cost beside a query's cost on the
  same index, and a single resolved seed is the cheapest honest form of that.

## How to rerun

```console
$ bench/clone-corpus.sh csharp /tmp/corpus/csharp
$ bench/clone-corpus.sh typescript /tmp/corpus/typescript
$ cd /tmp/corpus/csharp && devscout init --no-hooks --no-map && devscout map .
$ /usr/bin/time -l devscout export > /dev/null
$ /usr/bin/time -l devscout refs IBus --json > /dev/null
```

The same four commands per corpus, with the seed the table above names. Map runs
are not measured here; they are recorded by the extraction benchmarks.
