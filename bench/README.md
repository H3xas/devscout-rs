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

## Rules the harness holds to

- One clone per arm. This tool writes artifacts into the git common directory, so worktrees of
  one clone share state and would void a run.
- `origin` is removed from every clone. An arm that can fetch upstream reads the answer instead
  of finding it.
- Every arm's stdout and stderr are stored verbatim before anything is scored.
- Results measured on corpora that cannot be published are not published — not in summary, not
  as a ratio, not as a range.
