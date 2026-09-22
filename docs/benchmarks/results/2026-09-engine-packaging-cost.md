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
Date            2026-09-21
Corpus          fixtures/csharp-compiler-facts (this repository's own pinned fixture; not a
                cloned corpus -- the fixture that exercises the new protocol mode, not a
                large-solution stand-in)
Tool version    devscout 0.6.0 (crate version; source tree carries no git metadata, so no git
                describe), built at a9f4b7758c83ade05b9af25599c94ec2d5ccdfef
Build           cargo build --release --locked, rustc 1.97.1 (8bab26f4f 2026-07-14), cargo
                1.97.1 (c980f4866 2026-06-30), aarch64-apple-darwin -- captured by the bench
                script itself (`bench/compiler-facts-cost.sh`'s own toolchain block), not
                transcribed by hand
Engine          tools/scout-semantic 0.1.0, Roslyn (Microsoft.CodeAnalysis.CSharp.Workspaces)
                4.14.0, Microsoft.Build.Locator 1.9.1 (tools/scout-semantic/packages.lock.json)
SDK             dotnet 9.0.305 (msbuild 9.0.305)
Host            macOS (Darwin 25.2.0), arm64, otherwise idle; 12 cores, 64 GiB RAM. Kernel
                name/release/machine only (`uname -srm`); the bench script never captures the
                host's network name.
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
| Default CLI binary size | 12,940,144 bytes (12.34 MiB) | 12,940,144 bytes (identical) |
| Default CLI `--version`, cold / warm ×3 | 0.0051s / 0.0044s 0.0043s 0.0044s | 0.0050s / 0.0048s 0.0047s 0.0046s |
| Framework-dependent engine publish | 35,838,445 bytes (34.18 MiB) | 35,838,445 bytes (identical) |
| Self-contained engine publish (osx-arm64) | 118,920,771 bytes (113.42 MiB) | 118,920,771 bytes (identical) |
| Compiler-facts wall time, cold | 1.943s | 1.887s |
| Compiler-facts wall time, warm ×3 | 1.905s, 1.883s, 1.953s | 1.929s, 1.924s, 1.904s |
| Peak engine memory (cold run, `/usr/bin/time -l` maximum resident set size) | 172,294,144 bytes (164.3 MiB) | 174,571,520 bytes (166.5 MiB) |
| Artifact import/admission cost (`compiler-facts import` on the committed fixture artifact, admitted outside a git checkout -- see Reading) | 0.018s | 0.019s |
| New CI steps, local proxy (fixture restore + engine run + `diff`, warm NuGet cache) | restore 0.402s, run 1.913s, diff 0.005s, total 2.320s | restore 0.387s, run 1.924s, diff 0.006s, total 2.316s |

## Reading the numbers against the predictions

- **Default CLI size (12.34 MiB) and startup (~4.5ms warm)** land inside the predicted range and
  are unaffected by this ticket by design: `Cargo.toml`'s `exclude` already keeps `tools/` out of
  the published crate, and `tests/cargo_publish_excludes_engine.rs` asserts that mechanically.
  Both committed runs, taken after the binary already existed on disk, show cold and warm within
  noise of each other; a fresh-binary disk-cache miss immediately after `cargo build --release`
  finishes linking can push one isolated cold measurement well above this, which is why the
  script is always run against an already-built binary.
- **Framework-dependent (34.18 MiB) and self-contained (113.42 MiB) engine publishes** both land
  inside their predicted ranges. The self-contained variant adds almost exactly the runtime-bundle
  cost the prediction named (~83 MiB here). Neither variant is committed to by this record — no
  packaging variant has been approved, and both figures are reported side by side for that
  decision.
- **Cold and warm compiler-facts wall time land inside the predicted band, and the "no
  improvement over cold" prediction held**: warm repeats (1.88–1.95s) do not undercut cold
  (1.89–1.94s) by any consistent margin. This confirms the "no persistent compiler daemon"
  decision has the wall-time consequence the Design's own non-goal already named: every
  invocation pays approximately the same `MSBuildLocator`/workspace-load cost.
- **Peak memory (164.3–166.5 MiB)** lands inside the predicted range, essentially unchanged
  between runs — consistent with Roslyn/MSBuild's fixed workspace-loading cost dominating over
  this one-file fixture's own negligible size.
- **Artifact import/admission cost (18–19ms)** lands inside the "under 10ms of actual parse/write
  work" prediction's own end-to-end allowance once process-startup floor is accounted for, and —
  unlike an earlier measurement pass — this cell now times a real admission, not a refusal. The
  committed fixture artifact carries `sourceSnapshot: null` (it was produced with `--no-git`,
  intentionally source-agnostic per its own README), so importing it into a root that is itself a
  git checkout trips the source-snapshot identity check and times a refusal instead of an
  admission — exactly the failure mode `tests/compiler_facts_cli.rs`'s
  `import_inside_a_git_checkout_admits_a_matching_source_snapshot_and_refuses_a_mismatched_one`
  now covers directly. `bench/compiler-facts-cost.sh` therefore `init`s a fresh, throwaway
  directory with no git ancestor at all before importing into it, the same way a non-git
  deployment already would, and no longer discards the command's exit code: both runs above print
  `admitted compiler facts (engine 1), coverage: incomplete (1 unit affected)` in
  `bench/out/compiler-facts-cost/import.log`, matching the fixture's own deliberate `CS1061` (see
  `fixtures/csharp-compiler-facts/README.md`). The admission path's own pure `admit` function has
  no I/O and no subprocess; `src/graph/tests/compiler_facts.rs`'s unit tests exercise it directly
  without any process-spawn cost at all.
- **The new CI steps' local proxy (2.316–2.320s total)** lands inside the predicted "a few
  seconds" band, now measured twice rather than once. It measures a warm-NuGet-cache invocation,
  the same state the `semantic-audit` job reaches by the point these new steps run (after the
  existing oracle and flowtrace-facts steps have already restored/built); an actual CI run's added
  wall time was not separately measured (that needs a real workflow run, outside what a local
  script can observe) and may differ under CI's own I/O and CPU characteristics -- recorded as a
  local proxy, not a CI measurement, per the Deviations line above.

## What this does not measure

No numeric accept/reject threshold is set here — that is a release-candidate decision, an operator
call, not a conclusion this document draws. No packaging-variant selection is made. No
claim is carried over from the earlier toy fixtures referenced in the Design's own decisions: every
figure above is freshly measured against this ticket's own pinned fixture. The MassTransit corpus
benchmark family in `2026-09-resolver-precision.md` is unrelated: it scores resolver precision, not
packaging or engine-invocation cost, and this record does not touch it.

## 2026-09-22 — occurrence capability

`bench/compiler-facts-cost.sh` now measures two arms over the same pinned fixture: **default**
(no `--capabilities` flag — the engine's own unflagged default, which now includes per-reference
occurrence facts) and **restricted** (`--capabilities symbols,diagnostics` — the pre-occurrence
capability set). The figures above (September 21) measured an engine that had no occurrence
capability and no real embedded context envelope at all; this section supersedes them rather than
leaving them silently stale, and isolates what the occurrence walk itself costs on top of the
context-envelope-widening cost every mode now pays (`Runner.Run`'s `wantContext` gate change,
recorded as a non-blocking risk in the implementation plan).

### Environment

Captured by the script's own toolchain block, not transcribed by hand: `rustc: rustc 1.97.1
(8bab26f4f 2026-07-14)`, `cargo: cargo 1.97.1 (c980f4866 2026-06-30)`, `dotnet: 9.0.305`, `uname:
Darwin 25.2.0 arm64`. Tool version: devscout 0.6.0, built at `a0b59ac32d86792cae8ccbd57b2a09d20d4bc295`.
Same host, bench root, isolation and network posture as the September 21 run above. 2 full script
runs; wall-time cells additionally take 3 warm repeats within each run, per arm.

### Registered predictions

Written before this rerun, from the walker's own design (a single additional semantic-model pass
over already-loaded documents, no new project load, over this fixture's three small files) and the
existing September 21 baseline:

| Figure | Predicted |
| --- | --- |
| Cold/warm wall time, both arms | 2–4s, dominated by `MSBuildLocator`/workspace load as before; restricted at or a little below default (no occurrence walk), the gap small on a fixture this size |
| Peak engine memory, both arms | 150–300 MiB, within roughly 10 MiB of each other — occurrence records held in memory are negligible next to Roslyn/MSBuild's own fixed workspace cost |
| Admitted artifact size | Default noticeably larger than restricted (the occurrence payload plus the real embedded envelope, present in both); restricted still larger than the September 21 baseline (69 KiB before), since both arms now carry the real envelope instead of a two-field placeholder |
| Import/admission cost, both arms | Under 50ms end-to-end, dominated by process startup, roughly flat with artifact size at this scale |

### Measured

| Figure | Run 1, default | Run 1, restricted | Run 2, default | Run 2, restricted |
| --- | --- | --- | --- | --- |
| Wall time, cold | 2.310s | 2.374s | 2.364s | 2.328s |
| Wall time, warm ×3 | 2.358s, 2.350s, 2.343s | 2.338s, 2.387s, 2.325s | 2.402s, 2.393s, 2.387s | 2.352s, 2.331s, 2.310s |
| Peak engine memory (`/usr/bin/time -l` max RSS) | 214,630,400 bytes (204.7 MiB) | 213,532,672 bytes (203.6 MiB) | 214,581,248 bytes (204.6 MiB) | 214,581,248 bytes (204.6 MiB) |
| Admitted artifact size | 68,994 bytes (67.4 KiB) | 49,439 bytes (48.3 KiB) | 68,994 bytes (identical) | 49,439 bytes (identical) |
| Import/admission cost | 0.019s | 0.018s | 0.019s | 0.018s |

### Reading the numbers

- **Wall time lands inside the predicted band, and the two arms are indistinguishable from noise**
  (2.31–2.40s across both arms and both runs, no consistent ordering between default and
  restricted). The occurrence walk's own added cost is not visible at this fixture's size against
  the dominant `MSBuildLocator`/workspace-load floor — consistent with the September 21 record's
  own finding that warm never meaningfully undercuts cold for the same reason. A larger corpus
  would be needed to isolate the walk's own marginal cost from this floor; this record does not
  claim one.
- **Peak memory (203.6–204.7 MiB) lands inside the predicted band and the two arms agree within
  about 1 MiB** — Roslyn/MSBuild's own fixed workspace-loading cost dominates, exactly as
  predicted. This is somewhat above the September 21 baseline's 164–166 MiB, consistent with the
  `wantContext` gate widening: every compiler-facts run now also performs the context-envelope's
  own independent project-evaluation pass, on both arms equally.
- **Admitted artifact size**: default (67.4 KiB) versus restricted (48.3 KiB) confirms the
  occurrence payload's own footprint (roughly 20 KiB on this fixture); both are well above the
  September 21 baseline's 69 → now-comparable-but-structurally-different byte count — the prior
  figure measured a placeholder envelope with no occurrence capability at all, so it is a
  different artifact shape, not a smaller version of the same one. Both runs are byte-identical
  across the two repeats, matching the fixture's own two-runs-are-byte-identical property.
- **Import/admission cost (18–19ms, both arms, both runs)** lands inside the predicted band and is
  flat with respect to the roughly 20 KiB artifact-size difference between arms — consistent with
  process-startup, not JSON parse/write work, dominating this cell, the same reading the September
  21 record gave.
- No numeric budget, threshold or packaging-variant selection is set by this section, matching the
  standing Design decision.

## Rerunning

```sh
cargo build --release --locked
dotnet build tools/scout-semantic -c Release
sh bench/compiler-facts-cost.sh
```

Publish output and timing logs land under `bench/out/compiler-facts-cost/`, gitignored like every
other `bench/out/` artifact.
