# bench

The harness behind every number in [`docs/benchmarks/`](../docs/benchmarks/README.md). Nothing
here is wired into `cargo test`; it is run by hand, against a corpus pinned by SHA.

| File | What it is |
| --- | --- |
| `corpus.lock` | the pinned public corpora — repository, SHA, licence, status |
| `clone-corpus.sh` | shallow-clones one locked corpus at its pin and removes `origin` |

## Corpora

`corpus.lock` is the only place a pin is written. The C# corpus is registered; the TypeScript
one is a proposal and carries no SHA until the first run that measures against it records one.

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
row of a saved `refs --json` or `tests --json` answer against the LSIF references. See
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
