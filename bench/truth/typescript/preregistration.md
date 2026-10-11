# TypeScript reference parity — pre-registration

**Sealed before any arm runs.** Nothing in this directory is changed after the first arm output
is seen: not the seeds, not the truth, not the templates, not the thresholds, not the
predictions. A scorer defect found later is fixed in a new commit and disclosed in the results
document with its figures before and after the fix. A rerun under different rules is a new
directory and a new dated results document.

**Question.** Do `devscout refs`, `tests` and `impact` answer for TypeScript symbols as they do
for C# ones, and how does a TypeScript-capable peer answer the same questions on the same
corpus?

**Corpus.** `[typescript]` in [`bench/corpus.lock`](../../corpus.lock): nrwl/nx at
`1ce5e91f5ec13cf8812f3e67e1fae33079336876` (tag 22.5.4, MIT), fetched once by
`bench/clone-corpus.sh typescript <dir>`. At the pin it tracks 4052 `.ts` and 417 `.tsx` files,
233 of them `.d.ts`, and 986 test-path files. Its root `tsconfig.base.json` declares `baseUrl`
but no `paths`, and packages import each other by package name (`@nx/react/...`, `nx/src/...`,
`@nx/e2e-utils`).

## Files

| File | What it is |
| --- | --- |
| `seeds.json` | the seed rule's output: candidate counts, rejection counts, every pool in rank order, the five seeds, the declaring files and the diff list |
| `truth-raw.json` | for each seed and each exported name the impact targets declare: the files and line numbers ripgrep matched, minus the declaring files |
| `flags.json` | the mechanical adjudication flags over the raw truth |
| `adjudication.json` | one decision per flag, each with its reason, confirmed by reading the matched lines |
| `truth-sealed.json` | the raw truth minus the confirmed removals, with every removal and every overridden flag listed |
| `templates.tsv` | each arm's fixed invocation templates |
| `manifest.json` | sha256 of every file above plus the scripts, the peer requirements and this document |

The scripts are [`bench/ts_parity.py`](../../ts_parity.py) (tested by
[`bench/test_ts_parity.py`](../../test_ts_parity.py)) and the arm runner
[`bench/ts-parity.sh`](../../ts-parity.sh).

## Seed rule

Read from the source text by the bench's own export reader, never by devscout:

- **Candidates** are exported top-level declarations (`export` at column 0, followed by
  `function`, `class`, `interface`, `type`, `enum`, `namespace`, `const`, `let` or `var`) in
  tracked `.ts`/`.tsx` files that are neither test-path files nor `.d.ts` files.
- The name matches `^[A-Za-z_][A-Za-z0-9_]*$`, because `\b` does not bound `$`.
- The name is declared, by that same export reader, in exactly one tracked
  `.ts`/`.tsx`/`.js`/`.jsx` file.
- Its word-boundary file count — tracked `.ts`/`.tsx` files matching `\b<name>\b`, declaring
  file included — is between 5 and 200. The count comes from a word-run index of the corpus;
  every name that enters a pool is re-counted with ripgrep, and any disagreement aborts the
  selection.
- At least one test-path file other than the declaring file matches `\b<name>\b`.
- **Slots**, filled in this order from disjoint pools:
  - slot 5: hooks (a `use[A-Z]…` function) and components (a PascalCase function, or a
    PascalCase `const` initialised by a function or a call, in a `.tsx` file);
  - slot 4: interfaces and type aliases;
  - slots 1–3: the remaining functions (declarations, or `const`/`let`/`var` initialised by a
    function) and classes.
- In each pool, ascending sha256 of `<pin>:<name>` picks: the lowest for slots 4 and 5, the
  three lowest, in order, for slots 1–3.
- **Declaring files** are the files of seeds 1 and 4. **The diff list** is the files of seeds
  2, 3 and 5. The clone holds one revision, so "one diff" is this registered list of changed
  files.

Result: 311 function-and-class, 76 interface-and-type and 13 hook-and-component candidates.

| Slot | Seed | Declared in |
| --- | --- | --- |
| 1 | `ReleaseGraph` (class) | `packages/nx/src/command-line/release/utils/release-graph.ts` |
| 2 | `getReactDependenciesVersionsToInstall` (function) | `packages/react/src/utils/version-utils.ts` |
| 3 | `checkFilesMatchingPatternExist` (function) | `e2e/utils/file-utils.ts` |
| 4 | `FileMapCache` (interface) | `packages/nx/src/project-graph/nx-deps-cache.ts` |
| 5 | `Not` (component) | `nx-dev/feature-package-schema-viewer/src/lib/types/type.tsx` |

## Truth rule

- **Raw set** for a name: tracked `.ts`/`.tsx` files matching `\b<name>\b` with ripgrep 15.2.0,
  minus the files that declare it (for a seed, its declaring file; for an impact name, the
  target files that declare it).
- **Test-path rule:** a `.ts`/`.tsx` file named `*.spec.ts`, `*.spec.tsx`, `*.test.ts` or
  `*.test.tsx`, or with a directory segment exactly `__tests__`. `.js`/`.jsx` files never match.
- **Adjudication**, once, before any arm runs. The classifier flags a file when every match
  lies inside a comment (`comment only`), every match lies inside a comment or a string or
  template text with at least one in text (`string only`), or the file declares its own symbol
  of the same name (`other declaration`). Template `${…}` expressions count as code. Each flag
  was confirmed or overridden by reading its matched lines. 302 flags: 301 removals (153 string
  only, 121 comment only, 27 other declaration) and one override (a `type` import specifier the
  classifier read as a declaration). Nothing unflagged was removed.
- **Sealed set:** the raw set minus the confirmed removals. **Tests truth** is the sealed set
  restricted to test-path files. **Impact truth** for a file list is the union of the sealed
  sets of every exported top-level name those files declare, minus the listed files.

Sealed sizes: refs 8 + 6 + 5 + 4 + 0 = 23 files; tests 1 + 1 + 5 + 2 + 0 = 9 files. `Not` keeps
no file: every other mention of the word is comment or string text, and the component is used
only inside its own file. Impact truth: 16 files for `release-graph.ts`, 11 for
`nx-deps-cache.ts`, and 946 for the diff list, whose `e2e/utils/file-utils.ts` exports
general names (`readFile`, `exists`, `readJson`, …) that many unrelated files also use as
members or locals. The rule keeps those files, so the diff-list impact truth is wide and its
precision easy to reach; it is reported, not tuned.

## Metrics

- **Refs recall**, pooled over the five seeds (micro-average), with per-seed figures. A devscout
  answer's files are the distinct files of every row in every `inbound` table of
  `refs <id> --all --json`. A peer answer's files are the distinct files of its result rows.
- **Tests recall**, pooled the same way, over the distinct files of the `tests` rows.
- **Impact precision at depth 1**, on each declaring file and on the diff list: correct rows
  over returned rows, counted over the rows each answer returns at its default row cap (totals
  and truncation reported beside it), within the measured universe of `.ts`/`.tsx` files and
  with the queried files excluded. `.js`/`.jsx` rows are listed separately as outside the
  universe. An answer with no rows is published as "n/a (no rows)", which does not meet a
  threshold. devscout takes one seed per `impact` call, so its diff-list answer is the union of
  one depth-1 call per listed file; the peer takes the list in one call.
- Recall, default-depth `impact` and setup cost (build wall time) are reported as information.
- Every figure is published against the sealed truth and, for disclosure, the raw truth.

## Gate

Pre-registered. After the change, on this corpus and these seeds:

- pooled file-level `refs` recall ≥ 0.9;
- pooled `tests` recall ≥ 0.9;
- `impact` precision ≥ 0.5 at depth 1 on each declaring file and on the diff list.

A missed gate is published as a miss. Seeds, truth and templates are never changed after any
figure is seen.

## Arms

| Arm | What runs | Invocations |
| --- | --- | --- |
| `devscout-base` | devscout built from the commit before the change | `init --no-hooks`, then `refs <file>#<name> --all --json`, `tests <file>#<name> --json`, `impact <file> --hops 1 --json`, `impact <file> --json` |
| `devscout-head` | devscout built from the commit that carries the change | the same |
| `crg` | code-review-graph 2.3.7 from PyPI | `build`, `status --json`, then `query callers_of`, `query inheritors_of`, `query tests_for` on `<file>::<name>`, `impact --files <files> --depth 1`, `impact --files <files>` |

- Each arm addresses a seed by the most specific identifier its own help text offers: devscout's
  definition id `<file>#<name>`, the peer's qualified name `<file>::<name>`.
- The peer's refs answer is `callers_of` plus `inheritors_of`, its two symbol-level reference
  queries. `query importers_of` on the same target answers which files import the declaring
  file; it is run and reported as information ("refs widened by file importers"), never as the
  refs figure.
- `templates.tsv` is hashed in `manifest.json`; the runner refuses a template whose hash
  differs or whose verb is outside the arm's allowlist (devscout: `init`, `refs`, `tests`,
  `impact`; peer: `build`, `status`, `query`, `impact`).

**Isolation.** One clone per arm, copied from a pristine clone that is never indexed. The runner
starts each command from an empty environment holding only `PATH`, `LANG`, a scratch `HOME`
and the arm's scratch state (`SCOUT_REGISTRY` and `SCOUT_CONTENT_DB`, or `CRG_DATA_DIR`), and
refuses to run when any of them lies outside the scratch root or inside the real home
directory, when an environment name looks like a credential, when the corpus `HEAD` is not the
pin, or when the corpus sits inside a checkout of this repository. The peer is installed by
[`bench/peers/code-review-graph/install.sh`](../../peers/code-review-graph/install.sh) from
the hash-pinned [`requirements.txt`](../../peers/code-review-graph/requirements.txt): PyPI as
the only index, an empty package cache, `--require-hashes`, wheels only, no extras. The install
then refuses a package that lacks the upstream release's `skills.py`, `uninstall.py` and
`eval/`. No extra, embedding provider or `visualize` is used.

## Predictions

Written after reading the corpus layout and the sealed truth, before any arm runs.

| Arm | Metric | Prediction | Falsified if |
| --- | --- | --- | --- |
| `devscout-base` | refs, tests recall | 0: TypeScript edges are in the graph but no query reads them | any seed returns a row |
| `devscout-base` | impact precision, depth 1 | n/a (no rows) on every target | any target returns a row |
| `devscout-head` | refs recall | 0.65 (15 of 23): every relative-path importer is found; the 8 files that import a seed by package name (`@nx/react/src/...`, `@nx/e2e-utils`, `nx/src/...`) are missed, because no `paths` entry maps those names | pooled recall below 0.55 or above 0.75 |
| `devscout-head` | refs gate (≥ 0.9) | **missed** | pooled recall ≥ 0.9 |
| `devscout-head` | tests recall | 0.33 (3 of 9): the five end-to-end tests of `checkFilesMatchingPatternExist` import it by package name | pooled recall below 0.2 or above 0.5 |
| `devscout-head` | tests gate (≥ 0.9) | **missed** | pooled recall ≥ 0.9 |
| `devscout-head` | impact precision, depth 1 | ≥ 0.5 on both declaring files and on the diff list: every row is an import, call, JSX or dispatch edge to the file | any target below 0.5 or without rows |
| `crg` | refs recall (callers and inheritors) | ≤ 0.35: an interface, and a class used mostly as a type, have no call edges | pooled recall above 0.35 |
| `crg` | tests recall | ≤ 0.5 | pooled recall above 0.5 |
| `crg` | impact precision, depth 1 | ≥ 0.5 on the diff list, below 0.5 on at least one declaring file: its depth-1 neighbourhood includes nodes contained in and called from the changed file | the diff list below 0.5, or both declaring files at or above 0.5 |

## Where peers win

The results document generates this section mechanically: for every metric cell (pooled and
per-seed recall, per-target precision) it names the leading arm, ties naming every leader, and
lists every cell a peer leads. It is not edited by hand.
