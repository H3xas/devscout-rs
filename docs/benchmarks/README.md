# Benchmarks

Everything this project claims about its own speed, cost, or accuracy lives here, with the
command that produced it. Nothing else in `docs/` grows past reference minimum — benchmarks
are the one sanctioned place where new claims accumulate, and they carry their own discipline
in exchange for that room.

| Document | What it holds |
| --- | --- |
| [methodology.md](methodology.md) | Task taxonomy, metrics, environment disclosure, exact rerun steps |
| [peers.md](peers.md) | The other tools an agent could install instead, and where each one leads |
| [agentic.md](agentic.md) | The model-in-the-loop protocol: lanes, effort, arms, disclosure |
| [results/](results/) | Dated result documents. One file per run. Never edited in place. |

The harness itself is in [`bench/`](../../bench/README.md).

## The honesty statement

This is the part that governs the rest, so it is stated before any number exists.

**Gaps are published in both directions.** Every results document is required to name the
arm that leads each band × metric cell, including the cells where a peer or a plain
`ripgrep` baseline beats this tool. A results document with no "where peers win" section is
an incomplete results document, not a favourable one.

**No best-everywhere claim will be made here.** This index answers name-level and
reachability questions from a prebuilt graph. Questions that reduce to finding one
distinctive string are answered well, cheaply, and without any index at all by a skilled
agent holding `rg` — and a benchmark that hides that is measuring a strawman. Where the
index does not earn its cost, the results say so in those words.

**The baseline is skilled, not staged.** Order-of-magnitude wins in this space are usually
wins over a baseline that was told to read the repository or given no search strategy. The
baseline here gets ripgrep, listing, read, an explicit competent playbook, an equal budget,
and published transcripts. If this tool cannot beat that, it has no result worth publishing.

**Methodology and results are separate files, and results are dated.** Editing a method
never silently redates a number. A rerun gets a new file in `results/`; it does not overwrite
an old one.

**Pinned corpora, or no number.** Every figure is measured against a corpus pinned by SHA and
named in [`bench/corpus.lock`](../../bench/corpus.lock). Results measured on corpora that
cannot be published are not published — not in summary, not in ratio form, not as a range.

**Predictions are registered before the run and misses are published as misses.** The
falsification condition for each claim is written down first. A run that falsifies its own
claim is published with the same prominence as one that does not.

**Single-run figures carry a banner.** One repetition per cell cannot separate a small effect
from noise. Until a multi-run protocol closes it out, every single-run table says so at the
top of the document rather than in a footnote.

## Status

The first public-corpus run is in [`results/2026-08.md`](results/2026-08.md): scripted, no model
in the loop, against the pinned MassTransit corpus. It reports this tool losing four of eight
tasks to a skilled `rg` baseline and never breaking even on setup cost, because that is what it
measured. The agentic (model-in-the-loop) round is now appended to that same file — preliminary, one run
per cell, with its integrity caveats leading.
