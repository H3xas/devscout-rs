# Freshness-validation latency — 2026-10-14

What a query pays for certifying that the index still describes the working tree, against the
previous release in the same state. The harness is
[`bench/freshness-latency.sh`](../../../bench/freshness-latency.sh); the corpus and its pin are in
[`bench/corpus.lock`](../../../bench/corpus.lock).

## Setup

| Item | Value |
|---|---|
| Corpus | `typescript`, nrwl/nx at `1ce5e91f5ec13cf8812f3e67e1fae33079336876` (tag 22.5.4) |
| Seed | `createProjectGraphAsync` (56 inbound references on this corpus) |
| Branch arm | release build of `fix/worktree-owned-index-freshness` |
| Baseline arm | release build of tag `v0.9.0`, same toolchain and lock file |
| Timer | `hyperfine --warmup 20 --runs 200` over `devscout refs <seed> --json` |
| p95 | nearest-rank, the 190th of 200 sorted times |
| Machine | Apple M2 Max, macOS 26.2, arm64 |

Each arm was given its own registry, content store and copy of the clone. Both copies had their
Git index refreshed once before mapping — without that, `cp -R` leaves every copied file newer
than the copied index and every `git status` re-hashes the whole tree, which measures the copy
rather than the binary.

## States

- **clean** — the clone is mapped and nothing has been touched since.
- **dirty** — one comment line is appended to the first 1,000 `.ts` files by sorted path and the
  clone is mapped, then a second line is appended to the first 100 of them. The index certifies
  the first append; the second is the change the query has to notice.

## Results

| State | Branch p95 | Baseline p95 | Delta |
|---|---|---|---|
| clean | 172.0 ms | 288.6 ms | −116.6 ms |
| dirty | 191.4 ms | 296.1 ms | −104.7 ms |

The dirty state answered `stale` with `changedFiles: 100`, not `validation-budget`. The
candidate set was 1,000 paths totalling 3,953,533 bytes (3.77 MiB), inside the 16 MiB cap.

Both deltas are negative: this build is faster than the release it is measured against, in both
states, because freshness is validated once per invocation where the earlier release validated
it twice — `--json` and the stderr line each ran their own `git status`.

## What this bounds

The 50 ms budget applies to the dirty state and the 10 ms budget to the clean state, both against
the same state of the previous release. Both hold, with the branch ahead by more than 100 ms in
each.

The figures are specific to this corpus, this seed and a tree whose Git index has been refreshed
by ordinary use. They do not bound a query on a tree whose index is stale across thousands of
files: a `git status` that may not write the index back re-hashes every racy file on every call,
so an index left stale by something other than the user's own Git costs the same on each query.
At the benchmarked 1,000-candidate state that is the difference between 172 ms and 288 ms; at a
whole-tree-stale index it is about half a second per query, against about a tenth for a build
whose status may write the index. [Limitations](../../limitations.md) carries this as a known
gap.
