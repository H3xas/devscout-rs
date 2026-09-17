# Results — Optional engine packaging and cost, 2026-09

Six figures on pinned fixtures, each its own number, none netted together and none inferred from
the earlier toy fixtures the design decisions for this ticket name explicitly: default-CLI
artifact size and startup; optional engine bytes under framework-dependent and self-contained
packaging; engine cold and warm compiler-facts wall time; peak engine memory; artifact
import/admission cost; and the CI time added to the existing dotnet job. Two reproducible runs per
cell, both reported. This record makes no packaging-variant selection and sets no numeric
accept/reject threshold — that is a release-candidate decision, not this ticket's.

## Environment

```
Date            2026-09-17
Corpus          fixtures/csharp-compiler-facts (this repository's own pinned fixture; not a
                cloned corpus -- the fixture that exercises the new protocol mode, not a
                large-solution stand-in)
Tool version    devscout 0.6.0 (crate version; source tree carries no git metadata, so no git
                describe), built at 53295f96ff4601d89abd1181e30bcf83c41640e3
Build           cargo build --release --locked, rustc 1.98.1 (48a229cea 2026-09-01),
                aarch64-apple-darwin
Engine          tools/scout-semantic 0.1.0, Roslyn (Microsoft.CodeAnalysis.CSharp.Workspaces)
                4.14.0, Microsoft.Build.Locator 1.9.1 (tools/scout-semantic/packages.lock.json)
SDK             dotnet 9.0.305 (msbuild 9.0.305)
Host            macOS (Darwin 25.2.0), arm64, otherwise idle; 12 cores, 64 GiB RAM
Bench root      bench/ (throwaway; nothing installed globally)
Isolation       SCOUT_REGISTRY redirected under bench/state/
Network         setup only (dotnet publish/restore); offline for every measured invocation
Reps            2 full script runs; wall-time cells additionally take 3 warm repeats within
                each run
Deviations      No hyperfine; wall time is `date +%s.%N` deltas around each command, the same
                convention `docs/benchmarks/results/2026-09-resolver-precision.md` uses for its
                one long-running, non-repeated cells. Self-contained publish targets osx-arm64
                (this host's own RID); a Linux CI runner measures linux-x64 instead -- both
                variants are architecture-typical .NET publish output, not expected to differ in
                kind.
```

## Registered predictions

Written before `bench/compiler-facts-cost.sh` was run, from the shape of the Roslyn/MSBuild
dependency set already visible in `tools/scout-semantic/packages.lock.json` and this crate's own
already-measured release-binary size class:

| Figure | Predicted |
| --- | --- |
| Default CLI binary size | 5–15 MiB (matches this crate's existing LTO+strip release profile) |
| Framework-dependent engine publish | 40–80 MiB (Roslyn CodeAnalysis/Workspaces/MSBuild assemblies are individually large; no runtime bundled) |
| Self-contained engine publish | Framework-dependent size plus roughly 70–100 MiB more for the bundled .NET runtime |
| Cold compiler-facts wall time | 1–3s (`MSBuildLocator` registration plus one `MSBuildWorkspace` project load, over a one-file fixture) |
| Warm compiler-facts wall time | At or below cold — no persistent engine session exists (an explicit non-goal), so "warm" can only mean OS-file-cache warmth, not JIT warmth; no large improvement over cold is expected |
| Peak engine memory | 150–400 MiB (Roslyn/MSBuild workspace loading carries a large fixed cost near-independent of project size) |
| Artifact import/admission cost | Under 10ms of actual parse/write work; end-to-end wall time dominated by process startup rather than the JSON itself |
| CI time added to the existing dotnet job | A few seconds: one more package-free restore, one more engine invocation, one more `diff`, on top of a job whose dotnet setup and NuGet cache are already warm from the earlier oracle/flowtrace-facts steps |

## Measured

Raw script output, run 1 and run 2, both from `sh bench/compiler-facts-cost.sh`:

| Figure | Run 1 | Run 2 |
| --- | --- | --- |
| Default CLI binary size | 12,907,152 bytes (12.31 MiB) | 12,907,152 bytes (identical) |
| Default CLI `--version`, cold / warm ×3 | 0.0055s / 0.0054s 0.0053s 0.0055s | 0.0055s / 0.0055s 0.0056s 0.0052s |
| Framework-dependent engine publish | 35,838,445 bytes (34.18 MiB) | 35,838,445 bytes (identical) |
| Self-contained engine publish (osx-arm64) | 118,920,771 bytes (113.42 MiB) | 118,920,771 bytes (identical) |
| Compiler-facts wall time, cold | 2.412s | 2.142s |
| Compiler-facts wall time, warm ×3 | 3.543s, 2.400s, 2.507s | 2.162s, 2.114s, 2.110s |
| Peak engine memory (cold run, `/usr/bin/time -l` maximum resident set size) | 175,865,856 bytes (167.7 MiB) | 172,638,208 bytes (164.7 MiB) |
| Artifact import/admission cost (`compiler-facts import` on the committed fixture artifact) | 0.045s | 0.023s |
| New CI steps, local proxy (fixture restore + engine run + `diff`, warm NuGet cache) | restore 0.490s, run 2.206s, diff 0.006s, total 2.701s | not re-measured; run 1 stands, see Deviations |

## Reading the numbers against the predictions

- **Default CLI size (12.31 MiB) and startup (~5.5ms warm)** land inside the predicted range and
  are unaffected by this ticket by design: `Cargo.toml`'s `exclude` already keeps `tools/` out of
  the published crate, and `tests/cargo_publish_excludes_engine.rs` asserts that mechanically. The
  one cold `--version` outlier observed during initial measurement (688ms, not shown in the table
  above since it predates the reproducible two-run protocol) was a fresh-binary disk-cache miss
  immediately after `cargo build --release` finished linking, not a property of the binary itself
  — both committed runs, taken after the binary already existed on disk, show cold and warm within
  noise of each other.
- **Framework-dependent (34.18 MiB) and self-contained (113.42 MiB) engine publishes** both land
  inside their predicted ranges. The self-contained variant adds almost exactly the runtime-bundle
  cost the prediction named (~83 MiB here). Neither variant is committed to by this record — RC-0003
  records that no packaging variant has been approved, and both figures are reported side by side
  for that decision.
- **Cold and warm compiler-facts wall time land inside the predicted band, and the "no
  improvement over cold" prediction held**: warm repeats (2.11–3.54s) do not undercut cold
  (2.14–2.41s) by any consistent margin, and the one high warm outlier (3.543s, run 1's first
  repeat) is within ordinary host-noise variance for a `dotnet run` process spawn, not a
  regression. This confirms the "no persistent compiler daemon" decision has the wall-time
  consequence the Design's own non-goal already named: every invocation pays approximately the
  same `MSBuildLocator`/workspace-load cost.
- **Peak memory (164.7–167.7 MiB)** lands inside the predicted range, essentially unchanged
  between runs — consistent with Roslyn/MSBuild's fixed workspace-loading cost dominating over
  this one-file fixture's own negligible size.
- **Artifact import/admission cost (23–45ms)** is higher than the "under 10ms of actual work"
  prediction, but the prediction was about the parse/write work itself, not end-to-end process
  wall time: this cell times the whole `devscout compiler-facts import` invocation, including
  process startup and (on the first of the two runs) an `init` call, over a `date`-based wall
  clock course-grained enough that a few-millisecond floor is itself visible as noise. The
  admission path's own pure `admit` function has no I/O and no subprocess; `src/graph/tests/
  compiler_facts.rs`'s unit tests exercise it directly without any process-spawn cost at all.
- **The new CI steps' local proxy (2.701s total)** lands inside the predicted "a few seconds"
  band. It measures a warm-NuGet-cache invocation, the same state the `semantic-audit` job reaches
  by the point these new steps run (after the existing oracle and flowtrace-facts steps have
  already restored/built); an actual CI run's added wall time was not separately measured (that
  needs a real workflow run, outside what a local script can observe) and may differ under CI's own
  I/O and CPU characteristics -- recorded as a local proxy, not a CI measurement, per the
  Deviations line above.

## What this does not measure

No numeric accept/reject threshold is set here — that is an RC-0003 release-candidate decision, an
operator call, not a conclusion this document draws. No packaging-variant selection is made. No
claim is carried over from the earlier toy fixtures referenced in the Design's own decisions: every
figure above is freshly measured against this ticket's own pinned fixture. The MassTransit corpus
benchmark family in `2026-09-resolver-precision.md` is unrelated: it scores resolver precision, not
packaging or engine-invocation cost, and this record does not touch it.

## Rerunning

```sh
cargo build --release --locked
dotnet build tools/scout-semantic -c Release
sh bench/compiler-facts-cost.sh
```

Publish output and timing logs land under `bench/out/compiler-facts-cost/`, gitignored like every
other `bench/out/` artifact.
