# DS-01

**Corpus:** csharp (MassTransit) @ 855cf1752c94ca9498e0c45ce8d09fdc9e957dd6
**Source:** maintainer-authored ticket, permitted for this run under the 2026-08-25 sourcing
waiver in `docs/benchmarks/agentic.md`; not sourced from MassTransit's public issue tracker.

**Statement**

Courier routing-slip activities can be canceled mid-flight (host shutdown, receive timeout,
etc.), and the framework is supposed to convert an unexpected cancellation from either side of an
activity — the execute step and the compensate step — into the same well-defined failure the rest
of the pipeline expects, rather than letting a bare cancellation escape or getting silently
absorbed into a fault.

We've had a report that execute-side and compensate-side activities don't behave identically when
the activity's own logic throws `OperationCanceledException` — sometimes the routing slip
continues on as if the step had actually completed, sometimes it faults instead, and it isn't
obviously symmetric between the two directions.

Trace how each side of a courier activity classifies, logs and propagates an
`OperationCanceledException` raised by the activity's own logic (as distinct from a cancellation
of the surrounding consume operation itself), note any place execute-side and compensate-side
diverge — in that handling or in anything else exception-related — and write up what you find,
including whether each divergence is a bug or working as intended.

**Definition of done:** a written comparison identifying every point of divergence between the
two sides (or a clear statement that none exists), each one classified as bug or intentional, and
a patch for any divergence classified as a bug.
