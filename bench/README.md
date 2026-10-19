# bench

The harness behind every number in [`docs/benchmarks/`](../docs/benchmarks/README.md). Nothing
here is wired into `cargo test`; it is run by hand, against a corpus pinned by SHA.

| File | What it is |
| --- | --- |
| `corpus.lock` | the pinned public corpora — repository, SHA, licence, status |
| `clone-corpus.sh` | shallow-clones one locked corpus at its pin and removes `origin` |
| `ts_parity.py`, `ts-parity.sh` | the TypeScript reference parity run: seed rule, truth, arm runner and scorer ([pre-registration](truth/typescript/preregistration.md)) |
| `ts_resolution_sites.cjs` | TypeScript 5.9.3 truth for the import sites the extractor records |
| `ts_resolution_envelope.py` | configuration-only site classification and base/head scoring ([registration](truth/module-resolution-2/preregistration.md)) |
| `ts_resolution_envelope_report.py` | deterministic results with excluded sites and synthetic cases |
| `test_ts_resolution_envelope.py`, `test_ts_resolution_envelope_report.py` | classifier, scorer and renderer checks |
| `freshness-latency.sh` | what a query pays for certifying a dirty index, against the previous release at the same state |
| `hook-latency.sh` | first-read offer latency against an identical read with the offer suppressed |
| `peers/code-review-graph/` | the peer arm's hash-pinned requirements and its guarded install |

## Corpora

`corpus.lock` is the only place a pin is written. Both the C# corpus (MassTransit) and the
TypeScript corpus (nx at tag 22.5.4) are registered with a SHA; a corpus with status
`proposed` carries no SHA and no published number until the first run that measures against it
records one.

## Running a comparison

Build the binary, clone the corpus at its pin, and index it in an isolated state directory so
the measurement never touches your real registry or content store.

```sh
cargo build --release

./bench/clone-corpus.sh csharp bench/clones/csharp

export SCOUT_REGISTRY="$PWD/bench/state/repos.json"
export SCOUT_CONTENT_DB="$PWD/bench/state/content.db"
mkdir -p bench/state

./target/release/devscout map bench/clones/csharp
```

Record the cold build wall time and the on-disk size of the artifact directory — they are the
setup-cost row, and they are never amortised into a per-task figure.

## Timing

[`hyperfine`](https://github.com/sharkdp/hyperfine) is the suggested timer: it reports cold and
warm separately, which the methodology requires both of.

```sh
hyperfine --warmup 0 --runs 1 \
  './target/release/devscout refs IBusControl --json'

hyperfine --warmup 3 --runs 10 \
  './target/release/devscout refs IBusControl --json' \
  'rg -n "IBusControl" bench/clones/csharp'
```

The first invocation after a build is the cold figure; the median of the warmed repeats is the
warm figure. Publish both. Comparing against `rg` on the same corpus is the floor of the table
— the baseline is skilled, not staged, so give it the same corpus and the same question.

## Capturing payloads

Cost without a model in the loop is payload bytes, reported as the proxy it is:

```sh
./target/release/devscout impact src/MassTransit/IBus.cs --hops 2 --json | wc -c
```

`tokens = ceil(bytes / 4)`. Never place a byte-derived token figure in the same table as a
harness per-agent counter figure from an agentic lane.

## Freshness validation latency

`freshness-latency.sh` measures the one cost a query pays that a `map` does not: certifying that
the index still describes the working tree.

```sh
./bench/clone-corpus.sh typescript bench/clones/typescript
sh bench/freshness-latency.sh bench/clones/typescript [baseline-binary]
```

Each arm gets its own state directory and its own copy of the clone. Two states are measured with
`hyperfine --warmup 20 --runs 200` over `refs <seed> --json`: a clean tree, and a tree with the
first 1,000 `.ts` files dirty at map time and 100 of them edited again afterwards. The script
prints both arms' p95 for both states, the deltas, the dirty state's `freshness` object and the
candidate byte total, and exits non-zero when a delta exceeds its budget or the dirty state does
not answer `stale`.

## Read-hook offer latency

Run `sh bench/hook-latency.sh [binary]` to measure a release build on an isolated two-file C#
fixture. Without a binary argument the harness builds the current branch. Each invocation gets
a fresh session; preparation and output assertions run outside the timed command. Both arms
use the same payload, and the suppressed arm changes one identifier on disk to another of the
same length. A settle pass precedes the recorded `hyperfine --warmup 20 --runs 200` pass.
The harness prints the environment, binary digest, distributions and verdict, and fails when
an output assertion or a latency bound fails. Off-reference absolute latency is unbounded.
Dated figures live in [the results index](../docs/benchmarks/README.md#results).

## Resolver precision (semantic oracle)

A second harness, independent of the cost/wall-time one above: it scores devscout's
`uses-member` edges against a **compiler** ground truth instead of measuring retrieval cost.
The oracle is [`tools/scout-semantic`](../tools/scout-semantic/README.md), a Roslyn console
tool (not part of the crate build) that walks a compiled C# solution and emits one record per
member-access, conditional-access, or bare-invocation site. `devscout audit --semantic`
scores devscout's own edges against those records.

**Prerequisites**: .NET SDK 9.0.3xx (`dotnet --version`) in addition to the Rust toolchain,
and the target solution's own restore inputs (NuGet feeds reachable at restore time).

```sh
cargo build --release
./bench/clone-corpus.sh csharp bench/clones/csharp

bench/semantic.sh bench/clones/csharp MassTransit.sln -p:TargetFrameworks=net9.0
```

(The clone step is the same one under "Running a comparison" above, and to the same
destination — `clone-corpus.sh` refuses a destination that already exists, so run it once and
reuse that clone for both harnesses.)

This restores `MassTransit.sln` inside the corpus, runs the oracle to produce
`refs.jsonl`/`units.jsonl`, indexes the corpus with `devscout map`, and runs
`devscout audit --semantic` twice — text to `bench/out/semantic/MassTransit/audit.txt`,
`--json` to `audit.json`, alongside the oracle's own JSONL in the same directory.

[`fixtures/csharp-semantic/`](../fixtures/csharp-semantic) is the pinned, hand-built solution
this metric family is validated against: each source file's header comment names its case
letter (`a`–`g`, plus the partial-class, enum-member, and receiver-shape sub-cases) and the
defect it probes — external-receiver leaks, enclosing-namespace extension methods, chained and
`this`-qualified receivers, cross-project structural impossibility. Its `oracle/refs.jsonl` and
`oracle/units.jsonl` are a committed snapshot that CI diffs against a fresh oracle run on every
build, so a Roslyn or extractor regression there fails the build before it reaches a corpus.
See [`docs/benchmarks/methodology.md`](../docs/benchmarks/methodology.md#resolver-precision)
for the metric definitions, the join rule, and the two-run protocol.

**`refs.jsonl` for a private or unpublishable corpus is never committed** — the same rule as
every other corpus artifact in this directory.

## Rust edges (rust-analyzer oracle)

A third harness scores devscout's Rust `uses-member`, `uses-type` and `implements` edges, and
the test-to-function pairs they imply, against the references `rust-analyzer lsif` writes for
this repository itself. `rust_edges.py` and `rust_sites.py` are standard-library Python;
`rust-edges-sample/` holds a hand-written LSIF sample over two invented files, which the unit
tests read.

**Prerequisites**: the `rust-analyzer` component of the pinned toolchain (`rustup component
add rust-analyzer`, which also needs `rust-src` so std macros expand), every dependency already
in the local cargo cache, and a devscout release binary built from the commit being scored.

```sh
python3 -B -m unittest discover -s bench -p test_rust_edges.py

git archive <commit> | tar -x -C /tmp/build-<commit>
cargo build --release --offline --locked --manifest-path /tmp/build-<commit>/Cargo.toml

python3 -B bench/rust_edges.py run --repo . --commit <commit> \
  --devscout /tmp/build-<commit>/target/release/devscout \
  --rust-analyzer "$(rustup which rust-analyzer)" \
  --label <label> --scratch /tmp/rust-edges-<label>
```

`run` refuses a scratch directory inside a git work tree, because devscout climbs to the
nearest `.git` and would map the enclosing repository instead of the export. Every cargo and
rust-analyzer step runs with `CARGO_NET_OFFLINE=true` and a scratch `CARGO_TARGET_DIR`. The LSIF
dump, the compiler's `cargo test -- --list` output, the mapped `graph.json`, the logs and the
scored `report.json`, `edges.jsonl` (one verdict per Rust edge) and `references.jsonl` land in
`bench/out/rust-edges/<label>/`, which is ignored. `score --out <dir> --source <export>`
rescores saved files, and `confirm --out <dir> --source <export> <answer.json>...` checks each
row of a saved `refs --json` or `tests --json` answer against the LSIF references.
`report.json` lists under `calls.missed` every `[file, line, target]` population reference
that devscout's `uses-member` edges do not fully recall.

`--commit` takes any tree-ish `git archive` accepts, so a fixture package is judged on its own
by naming its directory: `--commit <commit>:fixtures/rust-targets` exports only that package,
and rust-analyzer, `cargo test -- --list` and devscout all run on it as a root of its own. See
[`docs/benchmarks/methodology.md`](../docs/benchmarks/methodology.md#rust-edges) for the
universe, the site classes and the thresholds.

## Rules the harness holds to

- One clone per arm. This tool writes artifacts into the git common directory, so worktrees of
  one clone share state and would void a run.
- `origin` is removed from every clone. An arm that can fetch upstream reads the answer instead
  of finding it.
- Every arm's stdout and stderr are stored verbatim before anything is scored.
- Results measured on corpora that cannot be published are not published — not in summary, not
  as a ratio, not as a range.
