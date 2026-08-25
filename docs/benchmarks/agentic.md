# Agentic lanes — protocol

> **Preliminary — 1 run.** Every agentic figure this project publishes today comes from a
> single repetition per cell. One rep cannot separate a small effect from run-to-run noise. The
> 5-run protocol will follow, and until it closes this out, every agentic table carries this
> banner at the top of its own document rather than in a footnote.

Protocol only. No lane has been run against a public corpus, and nothing here contains a
result.

## What the lanes measure

The peer comparison in [peers.md](peers.md) measures what a tool hands an agent for one
question — a scripted invocation, no model anywhere in it. That is deliberately not the same as
measuring what an agent *does* with the tool. These lanes measure the second thing: a model
solving a real open ticket end to end, with the index available and without it.

## The lanes

Two model lanes, both solving the same pre-registered open tickets against the same pinned
corpus, each run twice — once with the index, once without.

| Lane | Model | Reasoning effort |
| --- | --- | --- |
| A | Sonnet 5 | max |
| B | Opus 5 | xhigh |

| Arm | Toolset |
| --- | --- |
| `index` | this tool's CLI, plus read and listing |
| `none` | ripgrep, read, listing — no index |

Both lanes solve the same tickets independently; the two lanes are not averaged together and
are never reported as a single figure. Reasoning effort is pinned and identical across arms
inside a lane, and it is a strong-effort setting rather than a production default — these lanes
describe strong-effort agents, not typical ones, and the results document repeats that.

## Rules

- **Symmetric lanes.** One fresh worker per (ticket, lane, arm). Same template, same turn and
  tool-call budget, same task order. Only the tool section differs, written from that tool's own
  quickstart. All prompts published verbatim.
- **Budget.** 150k tokens per lane, hard stop. A lane that hits it is recorded as stopped, not
  retried.
- **The corpus predates the fix**, and each clone's `origin` is removed. Lanes get no web tool
  and no network: an arm that can open the upstream issue reads the answer instead of finding it.
- **Ground truth is the merged upstream fix**, recorded before any lane runs, sealed under
  `bench/truth/agentic/`.
- **2026-08-25 — ticket sourcing for run 1.** Maintainer-authored tickets are permitted in place
  of the open-issue-plus-merged-fix pipeline for this run; any results that use one disclose it.
  Real-issue, merged-fix sourcing remains the preferred pipeline starting with the next run.
- **Blind judging, run by Claude Fable 5.** A judge that ran no lane sees the ticket, the fix
  diff, and the lane outputs with arm labels stripped and order shuffled per ticket. The rubric
  is fixed first; arm identity is re-attached only after every verdict is in.
- **One clone per arm.** This tool writes artifacts into the git common directory, so worktrees
  of one clone share state and would void the run. Per-arm clone at the same commit, per-arm
  throwaway registry and content-store paths.
- **No commits inside lanes.** A lane may edit the tree; the diff is captured with `git diff`
  and the tree is reset. No lane commits, branches, or stashes.
- **Token accounting is harness accounting** — one per-agent counter for every cell, no
  byte-count proxy mixed in. Index build cost is reported separately with a break-even ticket
  count, never amortised into a per-ticket figure.
- **Misses are published**, including every ticket the `index` arm loses.

## Registered before the first lane

Written down before any lane starts, and published with the results whether or not they hold:

- Predicted median tokens per ticket and predicted correct-or-partial count, per lane per arm.
- The falsification condition — the specific cost margin and correctness margin whose failure
  refutes the claim under test.
- The void conditions. A `none`-with-no-repository control measures how much of the answer sits
  in model priors: if a no-repository arm scores near the index arm, the tickets were answerable
  without any tool and nothing is being measured. Public issues with public fixes may sit in
  training data; any ticket answered with no repository access is reported separately and
  excluded from the headline. This cannot be fully eliminated, and that stands.

## Scope of any claim these lanes can support

A lane result speaks to the ticket kinds it ran, at the effort it ran, on the corpus it ran, at
the repetition count it ran. It is not evidence about competing code-intelligence tools — those
are measured without a model in the loop, in [peers.md](peers.md), and the two sets of numbers
are never placed in the same table.
