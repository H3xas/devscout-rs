# TypeScript reference parity — 2026-09-27

> **Single run.** One repetition per cell. Recall and precision here are counts over a fixed, sealed
> truth set and do not vary between repetitions of the same binaries, but setup times are one
> measurement each.

**Question.** Do `devscout refs`, `tests` and `impact` answer for TypeScript symbols, before and
after the change that makes them read the TypeScript edge kinds, and how does a TypeScript-capable
peer answer the same questions on the same corpus? The seed rule, truth, adjudication, metrics,
gate, arms, isolation rules and predictions were fixed before any arm ran, in
[`bench/truth/typescript/preregistration.md`](../../../bench/truth/typescript/preregistration.md);
every file there still matches the hash `manifest.json` recorded for it.

**Gate verdict: missed.** The registered gate asked for pooled `refs` recall ≥ 0.9, pooled `tests`
recall ≥ 0.9 and depth-1 `impact` precision ≥ 0.5 on each declaring file and the diff list, all
after the change. `devscout-head` reaches **0.652** pooled `refs` recall (15 of 23) and **0.333**
pooled `tests` recall (3 of 9): both below their thresholds. Its `impact` precision is **1.000** on
all three targets, meeting that part. The two recall misses were predicted in the
pre-registration, and every missed file is an import written by package name (`@nx/react/…`,
`@nx/e2e-utils`, `nx/src/…`), which this corpus maps through no `tsconfig` `paths` entry and which
the change does not attempt to resolve.

## Environment

```
Corpus          nrwl/nx 1ce5e91f5ec13cf8812f3e67e1fae33079336876 (tag 22.5.4, MIT), bench/corpus.lock [typescript]
devscout-base   built from 8a37ab872e9f75e5c70e33a1d049cd8470cdf8d3, cargo build --release --locked, rustc 1.97.1, aarch64-apple-darwin
devscout-head   built from 2d79427db6418d0b15c10338fa27ae74ce349239, same toolchain
crg             code-review-graph 2.3.7 from PyPI, CPython 3.11.13, bench/peers/code-review-graph/requirements.txt
Truth           ripgrep 15.2.0, word-boundary pattern, adjudicated and sealed before any arm ran
Host            arm64 workstation, 12 cores, otherwise idle
Isolation       one copy of the corpus per arm ($CORPUS); every arm started from an empty environment with
                a scratch HOME and scratch state under $BENCH (SCOUT_REGISTRY/SCOUT_CONTENT_DB, CRG_DATA_DIR)
Network         the corpus clone and the peer install only; every measured invocation ran offline
Reps            1 per cell
Deviations      none
```

Setup cost, information only: `devscout init --no-hooks` mapped the corpus in 4.4 s (base) and
4.4 s (head), writing 14.3 MiB and 15.1 MiB of artifacts. `code-review-graph build` took 30.9 s and
wrote a 613 MiB data directory.

## Arms and invocations

The runner ([`bench/ts-parity.sh`](../../../bench/ts-parity.sh)) expanded the hashed templates in
[`bench/truth/typescript/templates.tsv`](../../../bench/truth/typescript/templates.tsv), with
`$CORPUS` the arm's copy of the corpus and `<id>` the seed's declaring file and name:

| Arm | Setup | Per seed | Per impact target |
| --- | --- | --- | --- |
| `devscout-base`, `devscout-head` | `devscout init --no-hooks` | `devscout refs <file>#<name> --all --json`; `devscout tests <file>#<name> --json` | `devscout impact <file> --hops 1 --json` and `devscout impact <file> --json`, once per file of the target |
| `crg` | `code-review-graph build --repo $CORPUS`; `code-review-graph status --repo $CORPUS --json` | `code-review-graph query callers_of <file>::<name> --repo $CORPUS`, the same with `inheritors_of`, `importers_of` and `tests_for` | `code-review-graph impact --files <files> --depth 1 --repo $CORPUS` and the same without `--depth` |

Every invocation exited 0, except the ten `devscout-base` `impact` calls, which exited 3 (zero hit)
as that binary answers any TypeScript seed. For `FileMapCache`, an interface, `crg` answered
`not_found` to all four queries: it records no node for an interface declaration. Every answer's
files are listed in the tables below; each arm's stored stdout was scored as written.

## Seeds

| Slot | Seed | Declared in |
| --- | --- | --- |
| 1 | `ReleaseGraph` (class) | `packages/nx/src/command-line/release/utils/release-graph.ts` |
| 2 | `getReactDependenciesVersionsToInstall` (function) | `packages/react/src/utils/version-utils.ts` |
| 3 | `checkFilesMatchingPatternExist` (function) | `e2e/utils/file-utils.ts` |
| 4 | `FileMapCache` (interface) | `packages/nx/src/project-graph/nx-deps-cache.ts` |
| 5 | `Not` (component) | `nx-dev/feature-package-schema-viewer/src/lib/types/type.tsx` |

Declaring files: those of seeds 1 and 4. Diff list: the files of seeds 2, 3 and 5. `Not` has no
file in its sealed truth — every other mention of the word is comment or string text — so it adds
nothing to either pooled recall.

## `refs` recall

Against the sealed truth:

| Seed | Truth files | `devscout-base` | `devscout-head` | `crg` |
| --- | --- | --- | --- | --- |
| `ReleaseGraph` | 8 | 0/8 (0.000) | 8/8 (1.000) | 0/8 (0.000) |
| `getReactDependenciesVersionsToInstall` | 6 | 0/6 (0.000) | 4/6 (0.667) | 6/6 (1.000) |
| `checkFilesMatchingPatternExist` | 5 | 0/5 (0.000) | 0/5 (0.000) | 5/5 (1.000) |
| `FileMapCache` | 4 | 0/4 (0.000) | 3/4 (0.750) | 0/4 (0.000) |
| `Not` | 0 | 0/0 (n/a) | 0/0 (n/a) | 0/0 (n/a) |
| **pooled** | 23 | **0/23 (0.000)** | **15/23 (0.652)** | **11/23 (0.478)** |

Against the raw ripgrep truth, for disclosure:

| Seed | Truth files | `devscout-base` | `devscout-head` | `crg` |
| --- | --- | --- | --- | --- |
| `ReleaseGraph` | 9 | 0/9 (0.000) | 8/9 (0.889) | 0/9 (0.000) |
| `getReactDependenciesVersionsToInstall` | 6 | 0/6 (0.000) | 4/6 (0.667) | 6/6 (1.000) |
| `checkFilesMatchingPatternExist` | 5 | 0/5 (0.000) | 0/5 (0.000) | 5/5 (1.000) |
| `FileMapCache` | 4 | 0/4 (0.000) | 3/4 (0.750) | 0/4 (0.000) |
| `Not` | 35 | 0/35 (0.000) | 0/35 (0.000) | 0/35 (0.000) |
| **pooled** | 59 | **0/59 (0.000)** | **15/59 (0.254)** | **11/59 (0.186)** |

`code-review-graph`'s refs answer is `callers_of` plus `inheritors_of`. Widened with
`importers_of` on the same target, which answers which files import the declaring file rather than
who references the symbol, its pooled recall is 0.826 (19 of 23); this is information, never the
refs figure.

## `tests` recall

Against the sealed truth:

| Seed | Truth files | `devscout-base` | `devscout-head` | `crg` |
| --- | --- | --- | --- | --- |
| `ReleaseGraph` | 1 | 0/1 (0.000) | 1/1 (1.000) | 1/1 (1.000) |
| `getReactDependenciesVersionsToInstall` | 1 | 0/1 (0.000) | 1/1 (1.000) | 1/1 (1.000) |
| `checkFilesMatchingPatternExist` | 5 | 0/5 (0.000) | 0/5 (0.000) | 0/5 (0.000) |
| `FileMapCache` | 2 | 0/2 (0.000) | 1/2 (0.500) | 0/2 (0.000) |
| `Not` | 0 | 0/0 (n/a) | 0/0 (n/a) | 0/0 (n/a) |
| **pooled** | 9 | **0/9 (0.000)** | **3/9 (0.333)** | **2/9 (0.222)** |

Against the raw ripgrep truth:

| Seed | Truth files | `devscout-base` | `devscout-head` | `crg` |
| --- | --- | --- | --- | --- |
| `ReleaseGraph` | 2 | 0/2 (0.000) | 1/2 (0.500) | 2/2 (1.000) |
| `getReactDependenciesVersionsToInstall` | 1 | 0/1 (0.000) | 1/1 (1.000) | 1/1 (1.000) |
| `checkFilesMatchingPatternExist` | 5 | 0/5 (0.000) | 0/5 (0.000) | 0/5 (0.000) |
| `FileMapCache` | 2 | 0/2 (0.000) | 1/2 (0.500) | 0/2 (0.000) |
| `Not` | 5 | 0/5 (0.000) | 0/5 (0.000) | 0/5 (0.000) |
| **pooled** | 15 | **0/15 (0.000)** | **3/15 (0.200)** | **3/15 (0.200)** |

## `impact` precision at depth 1

Against the sealed truth; rows outside the `.ts`/`.tsx` universe were zero for every answer:

| Target | Truth files | `devscout-base` precision (correct/rows) | `devscout-head` precision (correct/rows) | `crg` precision (correct/rows) |
| --- | --- | --- | --- | --- |
| `packages/nx/src/command-line/release/utils/release-graph.ts` | 16 | n/a (no rows) | 1.000 (15/15) | 0.577 (15/26) |
| `packages/nx/src/project-graph/nx-deps-cache.ts` | 11 | n/a (no rows) | 1.000 (6/6) | 0.375 (6/16) |
| diff list | 946 | n/a (no rows) | 1.000 (9/9) | 0.692 (9/13) |

Against the raw truth:

| Target | Truth files | `devscout-base` precision (correct/rows) | `devscout-head` precision (correct/rows) | `crg` precision (correct/rows) |
| --- | --- | --- | --- | --- |
| `packages/nx/src/command-line/release/utils/release-graph.ts` | 16 | n/a (no rows) | 1.000 (15/15) | 0.577 (15/26) |
| `packages/nx/src/project-graph/nx-deps-cache.ts` | 16 | n/a (no rows) | 1.000 (6/6) | 0.375 (6/16) |
| diff list | 1159 | n/a (no rows) | 1.000 (9/9) | 0.692 (9/13) |

At the default depth, information only (`devscout` two hops, `code-review-graph` two hops, capped
at 500 nodes and truncated on all three targets):

| Target | Truth files | `devscout-base` precision (correct/rows) | `devscout-head` precision (correct/rows) | `crg` precision (correct/rows) |
| --- | --- | --- | --- | --- |
| `packages/nx/src/command-line/release/utils/release-graph.ts` | 16 | n/a (no rows) | 0.484 (15/31) | 0.041 (8/195) |
| `packages/nx/src/project-graph/nx-deps-cache.ts` | 11 | n/a (no rows) | 0.273 (6/22) | 0.033 (6/182) |
| diff list | 946 | n/a (no rows) | 0.750 (12/16) | 0.733 (118/161) |

The diff-list truth is wide by rule: `e2e/utils/file-utils.ts` exports general names (`readFile`,
`exists`, `readJson`, …) that many unrelated files also use as members or locals, and the truth
rule keeps them.

## Predictions against actual

| Arm | Metric | Predicted | Actual | Verdict |
| --- | --- | --- | --- | --- |
| `devscout-base` | refs, tests recall | 0 | 0.000, 0.000 | held |
| `devscout-base` | impact precision, depth 1 | n/a (no rows) on every target | n/a on all three | held |
| `devscout-head` | refs recall | 0.65 (15 of 23), falsified below 0.55 or above 0.75 | 0.652 (15 of 23) | held |
| `devscout-head` | refs gate (≥ 0.9) | missed | 0.652 | held: **gate missed** |
| `devscout-head` | tests recall | 0.33 (3 of 9), falsified below 0.2 or above 0.5 | 0.333 (3 of 9) | held |
| `devscout-head` | tests gate (≥ 0.9) | missed | 0.333 | held: **gate missed** |
| `devscout-head` | impact precision, depth 1 | ≥ 0.5 on all three targets | 1.000, 1.000, 1.000 | held: impact gate met |
| `crg` | refs recall | ≤ 0.35 | 0.478 | **falsified** |
| `crg` | tests recall | ≤ 0.5 | 0.222 | held |
| `crg` | impact precision, depth 1 | ≥ 0.5 on the diff list, below 0.5 on at least one declaring file | 0.692; 0.577 and 0.375 | held |

The falsified prediction: `code-review-graph` resolves a call by name across package boundaries,
so every caller of the two cross-package functions was found, where the prediction had counted
only its missing type references.

## Misses and extras, classified

Misses against the sealed truth:

| Arm | Seed | Missed files |
| --- | --- | --- |
| `devscout-head` refs | `getReactDependenciesVersionsToInstall` | `packages/next/src/generators/application/application.ts`<br>`packages/next/src/generators/init/init.ts` |
| `devscout-head` refs | `checkFilesMatchingPatternExist` | `e2e/dotnet/src/dotnet-advanced-msbuild.test.ts`<br>`e2e/dotnet/src/dotnet-cache.test.ts`<br>`e2e/dotnet/src/dotnet-configurations.test.ts`<br>`e2e/dotnet/src/dotnet-multi-project.test.ts`<br>`e2e/dotnet/src/dotnet.test.ts` |
| `devscout-head` refs | `FileMapCache` | `packages/angular/src/migrations/update-21-0-0/change-data-persistence-operators-imports-to-ngrx-router-store-data-persistence.spec.ts` |
| `devscout-head` tests | `checkFilesMatchingPatternExist` | `e2e/dotnet/src/dotnet-advanced-msbuild.test.ts`<br>`e2e/dotnet/src/dotnet-cache.test.ts`<br>`e2e/dotnet/src/dotnet-configurations.test.ts`<br>`e2e/dotnet/src/dotnet-multi-project.test.ts`<br>`e2e/dotnet/src/dotnet.test.ts` |
| `devscout-head` tests | `FileMapCache` | `packages/angular/src/migrations/update-21-0-0/change-data-persistence-operators-imports-to-ngrx-router-store-data-persistence.spec.ts` |
| `crg` refs | `ReleaseGraph` | `packages/nx/src/command-line/release/changelog.ts`<br>`packages/nx/src/command-line/release/command-object.ts`<br>`packages/nx/src/command-line/release/utils/resolve-semver-specifier.ts`<br>`packages/nx/src/command-line/release/utils/shared.spec.ts`<br>`packages/nx/src/command-line/release/utils/shared.ts`<br>`packages/nx/src/command-line/release/version.ts`<br>`packages/nx/src/command-line/release/version/derive-specifier-from-conventional-commits.ts`<br>`packages/nx/src/command-line/release/version/release-group-processor.ts` |
| `crg` refs | `FileMapCache` | `packages/angular/src/migrations/update-21-0-0/change-data-persistence-operators-imports-to-ngrx-router-store-data-persistence.spec.ts`<br>`packages/nx/src/daemon/server/project-graph-incremental-recomputation.ts`<br>`packages/nx/src/project-graph/build-project-graph.ts`<br>`packages/nx/src/project-graph/nx-deps-cache.spec.ts` |
| `crg` tests | `checkFilesMatchingPatternExist` | `e2e/dotnet/src/dotnet-advanced-msbuild.test.ts`<br>`e2e/dotnet/src/dotnet-cache.test.ts`<br>`e2e/dotnet/src/dotnet-configurations.test.ts`<br>`e2e/dotnet/src/dotnet-multi-project.test.ts`<br>`e2e/dotnet/src/dotnet.test.ts` |
| `crg` tests | `FileMapCache` | `packages/angular/src/migrations/update-21-0-0/change-data-persistence-operators-imports-to-ngrx-router-store-data-persistence.spec.ts`<br>`packages/nx/src/project-graph/nx-deps-cache.spec.ts` |

- **`devscout-head`, every miss: unresolved specifier.** Each missed file imports the seed by
  package name: `@nx/react/src/utils/version-utils` (the two `packages/next` files),
  `@nx/e2e-utils` (the five end-to-end tests), `nx/src/project-graph/nx-deps-cache` (the migration
  spec). The corpus declares no `paths` for its own packages, and resolving a package through its
  manifest or workspace layout is not part of this change. These are the same eight files the
  pre-registration named.
- **`crg`, `ReleaseGraph` refs: no call edge to a type.** The eight files import the class and
  use it as a type; `callers_of` and `inheritors_of` return nothing for it.
- **`crg`, `FileMapCache` refs and tests: no node.** The peer records no node for an interface
  declaration and answered `not_found`.
- **`crg`, `checkFilesMatchingPatternExist` tests: no test link.** Its callers are the five
  end-to-end tests themselves, but `tests_for` returned no row for the seed.

Extras in `refs` answers — files not in the sealed truth:

| Arm | Seed | Extra files |
| --- | --- | --- |
| `devscout-head` | `ReleaseGraph` | `packages/nx/src/command-line/release/utils/release-graph.ts` |
| `devscout-head` | `Not` | `nx-dev/feature-package-schema-viewer/src/lib/types/type.tsx` |
| `crg` | `Not` | `nx-dev/feature-package-schema-viewer/src/lib/types/type.tsx` |

Each extra is the seed's own declaring file: a reference from inside the file that declares the
symbol, which the truth rule excludes by construction. None is a wrong reference.

Extras in depth-1 `impact` answers: `devscout-head` had none.

| Arm | Target | Extra files |
| --- | --- | --- |
| `crg` | `packages/nx/src/command-line/release/utils/release-graph.ts` | `packages/nx/src/command-line/release/config/config.ts`<br>`packages/nx/src/command-line/release/config/filter-release-groups.ts`<br>`packages/nx/src/command-line/release/utils/git.ts`<br>`packages/nx/src/command-line/release/utils/repository-git-tags.ts`<br>`packages/nx/src/command-line/release/version/project-logger.ts`<br>`packages/nx/src/command-line/release/version/topological-sort.ts`<br>`packages/nx/src/config/project-graph.ts`<br>`packages/nx/src/generators/tree.ts`<br>`packages/nx/src/project-graph/affected/affected-project-graph.ts`<br>`packages/nx/src/project-graph/file-utils.ts`<br>`packages/nx/src/utils/command-line-utils.ts` |
| `crg` | `packages/nx/src/project-graph/nx-deps-cache.ts` | `packages/nx/src/config/nx-json.ts`<br>`packages/nx/src/config/project-graph.ts`<br>`packages/nx/src/config/workspace-json-project-json.ts`<br>`packages/nx/src/daemon/is-on-daemon.ts`<br>`packages/nx/src/daemon/logger.ts`<br>`packages/nx/src/project-graph/error-types.ts`<br>`packages/nx/src/project-graph/utils/project-configuration-utils.ts`<br>`packages/nx/src/utils/cache-directory.ts`<br>`packages/nx/src/utils/fileutils.ts`<br>`packages/nx/src/utils/versions.ts` |
| `crg` | `diff list` | `e2e/utils/get-env-info.ts`<br>`nx-dev/feature-package-schema-viewer/src/lib/types/join-types.tsx`<br>`nx-dev/feature-package-schema-viewer/src/lib/types/type-inference.ts`<br>`packages/react/src/utils/versions.ts` |

Every `crg` extra is a module the queried file itself imports: its depth-1 neighbourhood follows
edges in both directions, so it returns what a file depends on as well as what depends on it.

## Where peers win

Generated from the scorer's `wherePeersWin` output: every metric cell, the arm or arms leading it,
and each arm's value.

| Metric cell | Leading arm(s) | `devscout-base` | `devscout-head` | `crg` |
| --- | --- | --- | --- | --- |
| refs recall (pooled) | `devscout-head` | 0.000 | 0.652 | 0.478 |
| tests recall (pooled) | `devscout-head` | 0.000 | 0.333 | 0.222 |
| refs recall, FileMapCache | `devscout-head` | 0.000 | 0.750 | 0.000 |
| tests recall, FileMapCache | `devscout-head` | 0.000 | 0.500 | 0.000 |
| refs recall, Not | none (no truth) | n/a | n/a | n/a |
| tests recall, Not | none (no truth) | n/a | n/a | n/a |
| refs recall, ReleaseGraph | `devscout-head` | 0.000 | 1.000 | 0.000 |
| tests recall, ReleaseGraph | `crg`, `devscout-head` | 0.000 | 1.000 | 1.000 |
| refs recall, checkFilesMatchingPatternExist | `crg` | 0.000 | 0.000 | 1.000 |
| tests recall, checkFilesMatchingPatternExist | `crg`, `devscout-base`, `devscout-head` | 0.000 | 0.000 | 0.000 |
| refs recall, getReactDependenciesVersionsToInstall | `crg` | 0.000 | 0.667 | 1.000 |
| tests recall, getReactDependenciesVersionsToInstall | `crg`, `devscout-head` | 0.000 | 1.000 | 1.000 |
| impact precision depth 1, declaring file packages/nx/src/command-line/release/utils/release-graph.ts | `devscout-head` | n/a | 1.000 | 0.577 |
| impact precision depth 1, declaring file packages/nx/src/project-graph/nx-deps-cache.ts | `devscout-head` | n/a | 1.000 | 0.375 |
| impact precision depth 1, diff | `devscout-head` | n/a | 1.000 | 0.692 |

Cells a peer leads or ties:

- **tests recall, ReleaseGraph**: tie, crg, devscout-head at 1.000.
- **refs recall, checkFilesMatchingPatternExist**: `crg` 1.000 against `devscout-head` 0.000.
- **tests recall, checkFilesMatchingPatternExist**: tie, crg, devscout-base, devscout-head at 0.000.
- **refs recall, getReactDependenciesVersionsToInstall**: `crg` 1.000 against `devscout-head` 0.667.
- **tests recall, getReactDependenciesVersionsToInstall**: tie, crg, devscout-head at 1.000.

`code-review-graph` answers "who calls this function" across package boundaries that
`devscout` leaves unresolved on this corpus; it leads both cross-package seeds' refs cells.
`devscout-head` leads every depth-1 impact cell and every cell whose references are type imports.

## Rerunning

The steps are in
[`methodology.md` § TypeScript reference parity](../methodology.md#typescript-reference-parity).
