# Agentic pre-registration — devscout-rs

**Sealed:** 2026-08-25, before any lane has run. Predictions in this document are not adjusted
after any lane result is seen; a rerun of any part of this document is a new document, not an
edit to this one.

**Scope:** the 4 sealed tickets in this directory (DS-01..DS-04) against the csharp corpus
(MassTransit @ 855cf1752c94ca9498e0c45ce8d09fdc9e957dd6), under the 2 lanes defined in
[`docs/benchmarks/agentic.md`](../../../docs/benchmarks/agentic.md) — lane A (Sonnet 5, `max`),
lane B (Opus 5, `xhigh`) — each run twice, once per arm (`index`, `none`). Budget: 150k tokens
per lane, hard stop.

**Ticket sourcing disclosure:** all 4 tickets are maintainer-authored, under the 2026-08-25
sourcing waiver recorded in `docs/benchmarks/agentic.md`. This is disclosed here again because
it bears directly on the void-control prediction below — a maintainer-authored ticket has no
public issue/fix pair for a model to have memorized, which is expected to push every lane's
`void` score down relative to what a real, previously-public issue might score.

## Why the corpus shape drives these predictions

The csharp corpus is large and its `SqlTransport` area in particular is deep and repetitive
(~140 files, with a nested `SqlTransport/SqlTransport/...` namespace-mirroring directory
structure). Predictions below weight the `index` arm's advantage by how much a ticket's answer
is buried in that shape versus how much it depends on reading a small, already-located file
carefully.

## Predicted median tokens per ticket per lane×arm

| ticket | A/`index` | A/`none` | B/`index` | B/`none` |
|---|---|---|---|---|
| DS-01 | 30k | 34k | 45k | 50k |
| DS-02 | 38k | 45k | 55k | 65k |
| DS-03 | 34k | 42k | 50k | 60k |
| DS-04 | 40k | 70k | 58k | 95k |

Rationale: DS-01 truth is two files, both already named indirectly by domain vocabulary in the
ticket ("execute step"/"compensate step") — predicted the smallest `index` advantage, since a
skilled `rg` for `ExecuteActivityHost`/`CompensateActivityHost` finds them almost as fast as a
graph query would. DS-04 is predicted the largest `index` advantage by far: the truth file
(`SqlReceiveLockContext.cs`) sits among ~140 `SqlTransport` files with heavy naming overlap
(`Sql*Context`, `Sql*Configuration` repeated at multiple nesting levels), which is exactly the
shape where a `none` arm is predicted to spend many extra `rg`/`read` cycles narrowing down the
right file before it can even start reasoning about the `IRetryPolicy` relationship. DS-02 and
DS-03 fall in between — locating the trio pattern or the batching files is moderately easier with
`refs`/`impact` than with `rg` alone, but not as lopsided as DS-04's dense namespace-mirrored
tree.

## Predicted correct-or-partial count (out of 4 tickets), per lane×arm

| lane×arm | predicted correct-or-partial |
|---|---|
| A/`index` | 3 / 4 |
| A/`none` | 2 / 4 |
| B/`index` | 4 / 4 |
| B/`none` | 3 / 4 |

Rationale: all four tickets are investigation-shaped (write up what you find, patch only the
part classified as a bug/gap) rather than single-known-fix patches, so correctness depends more
on noticing the right asymmetry than on typing a specific line — predicted harder to swing
correctness with the index alone than to swing tokens. DS-04 is predicted the ticket most likely
to separate `index` from `none` on correctness specifically: `none` is predicted more likely to
either give up before finding `SqlReceiveLockContext.cs` inside the budget, or to substitute a
plausible-sounding but wrong file (e.g. mistaking `SqlHostConfiguration.cs`'s
`ReceiveTransportRetryPolicy` for the message-redelivery mechanism, which is exactly the
conflation the truth file calls out as a `partial`/`wrong` band).

## Falsification margins

Stated before any lane runs; a margin's failure is recorded against the tool, not quietly
dropped.

1. **Correctness margin.** For devscout-rs to be credited with a real correctness effect in a
   given lane, `index`'s correct-or-partial count must exceed `none`'s by at least 1 (out of 4
   tickets), for that lane. Predicted to hold for both lane A (3 vs 2) and lane B (4 vs 3) —
   stated as a real prediction, not a hedge, so it can be checked against directly.
2. **Token margin.** For devscout-rs to be credited with a real cost effect, `index`'s median
   tokens must be at least 20% lower than `none`'s, on the same ticket, for at least 3 of the 4
   tickets. Predicted to hold clearly on DS-04, plausibly on DS-02/DS-03, and marginally or not
   at all on DS-01 (see rationale above) — so the aggregate margin is predicted to hold, but not
   uniformly across every ticket, and that non-uniformity is itself part of what gets reported.
3. **Failure of either margin** on a lane is recorded, verbatim, as: "the index gave the
   `index` arm no measurable [correctness|cost] advantage over `none` on this ticket set, at this
   effort level, in this run" — with whichever bracket applies. That sentence is pre-written here
   so it cannot be softened after the fact if it is what happens.

## Void-control design

Per the void-condition requirement in `docs/benchmarks/agentic.md` ("Registered before the first
lane" section): a `none`-with-no-repository control measures how much of the answer sits in
model priors.

**Design:** for each of the 4 tickets, one additional lane — `void` — receives the exact ticket
statement and preamble used by the `none` arm, but no corpus clone, no `rg`, no file read, no
shell at all: the model answers from the prompt alone. The judge scores the `void` lane's answer
against the same sealed truth and rubric used for every other lane, blind, in the same batch, per
the existing blind-judging rule.

**Purpose:** if `void` scores near `none` or `index` on a ticket, the ticket was answerable
without any tool and nothing about that ticket is being measured; any ticket where that happens
is reported separately from the headline and excluded from it, per the existing void-condition
rule.

**Predicted `void` result:** 0 / 4 correct-or-partial. All four tickets are maintainer-authored
against a pinned MassTransit commit specifically so there is no public issue/fix pair for a
`void` lane to have memorized (per the ticket-sourcing disclosure above), and every truth file's
grading bands require a repo-relative file and specific code-grounded detail (e.g. the exact
missing guard in `CompensateActivityHost`, or the specific `IsReadyToDeliver` line) that is not
guessable from general MassTransit familiarity. A `void` score above 0/4 on any ticket is a
signal that ticket leaked more of its answer into its own statement than intended, and that
ticket's headline result is reported separately, not folded in, exactly as the rule requires.
