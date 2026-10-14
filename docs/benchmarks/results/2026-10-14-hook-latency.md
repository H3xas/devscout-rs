# First-read hook offer latency

**Single-run figures:** 200 measured invocations per arm after 20 warmups, with a separate
settle pass discarded. This synthetic two-file C# fixture measures the offer's cost; it does
not measure retrieval accuracy, compare peer tools, or establish a budget on another machine.

## Setup

| Input | Value |
| --- | --- |
| Machine | Apple M2 Max, 12 logical CPUs, local APFS disk |
| Platform | macOS 26.2 (25C56), arm64 |
| Build | Release profile, rustc 1.97.1 |
| Binary sha256 | `d204c2cea79b4cf921da172ee70628faadd9cc44b9732f5e87f4227ada5992b8` |
| Timer | hyperfine 2.0.0, default shell |
| Git | Apple Git 2.39.5, `/usr/bin/git` |
| Load averages, start | 3.67 / 4.78 / 4.86 |
| Load averages, end | 5.93 / 5.23 / 5.02 |
| Protocol | `sh bench/hook-latency.sh`, `--warmup 20 --runs 200` |
| Fixture | `IWidget.cs` and `WidgetImpl.cs`, the pair in `tests/freshness_fixture/mod.rs` |
| Isolation | Fresh fixture repository, home, registry and content store |

The page cache is warm. No build, test suite or other benchmark was run alongside the timer.
Each invocation receives a fresh session id. Preparation, file writes and output assertions
run outside the timed command. Both arms send the identical full-read payload: the indexed
body of `IWidget.cs`. The offer arm leaves those bytes on disk; the suppressed arm changes
`Render` to `Rendez`, preserving length while changing the digest. The second arm still pays
for process start, HEAD lookup, storage writes and every freshness check before the digest
mismatch suppresses the offer. Every offer output must contain `this file is indexed`;
every suppressed output must be empty. Stubs and HEAD-moved notes fail validation.

## Distributions

All values are milliseconds. p95 uses nearest rank, the 190th of 200 sorted measurements.
No outliers were removed.

| Arm | Minimum | p50 | p95 | Maximum | Mean | Validated invocations |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Offer | 21.043 | 22.671 | 26.491 | 38.942 | 23.195 | 220 / 220 |
| Suppressed | 19.791 | 22.605 | 26.322 | 44.349 | 23.076 | 220 / 220 |

The p95 delta is **+0.169 ms**, within the 5 ms bound. The offer's absolute p95 is
**26.491 ms**, within the 30 ms bound. Both gates pass; the harness exits zero.

## What this bounds

The offer adds little latency on this fixture and reference environment. The suppressed arm
wins the p95 comparison by 0.169 ms; this difference is small relative to timing noise.
No peer tool was measured, so these figures establish no advantage over a peer or `rg`.

Absolute latency outside this environment is unbounded: another platform, colder page cache,
slower disk or a more expensive Git invocation can cost more. A first read is roughly 20–27 ms
here, with one Git process contributing most of the cost. These figures do not support the
previous comment's claim of a first read below 10 ms.
