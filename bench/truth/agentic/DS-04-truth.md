# DS-04 — ground truth

**Corpus:** csharp (MassTransit) @ 855cf1752c94ca9498e0c45ce8d09fdc9e957dd6 (verified: paths
below exist at this sha, read directly from the local clone under `bench/corpora/csharp`; ticket's
loosely-specified "SqlTransport files + RetryPolicies/*" resolved to the specific paths below.)

## Truth files

- `src/MassTransit/SqlTransport/SqlTransport/SqlReceiveLockContext.cs` — implements
  `ScheduleRedelivery(TimeSpan delay, ...)`, the transport's delayed-redelivery mechanism.
- `src/MassTransit/SqlTransport/SqlTransport/Configuration/SqlHostConfiguration.cs` — declares
  `public override IRetryPolicy ReceiveTransportRetryPolicy { get; }` (constructed via
  `Retry.CreatePolicy(...)`).
- `src/MassTransit.Abstractions/Middleware/IRetryPolicy.cs` — the contract itself
  (`CreatePolicyContext<T>`, `IsHandled`).
- `src/MassTransit/RetryPolicies/` (dir) — the message-level policy implementations
  (`ImmediateRetryPolicy.cs`, `IncrementalRetryPolicy.cs`, `ExponentialRetryPolicy.cs`, etc.) that
  a message-retry pipeline (`UseMessageRetry`) would normally consult per attempt.

## What a correct investigation must identify

These are **two unrelated things that share the word "retry"**, and the ticket's premise —
that the two might disagree — resolves to "they don't overlap, so they can't disagree, but that's
worth stating explicitly because it's not obvious from the names":

- `SqlHostConfiguration.ReceiveTransportRetryPolicy` is an `IRetryPolicy` that governs retrying
  the **receive transport's own connection/operation failures** (broker connectivity), set up
  once per host via `Retry.CreatePolicy(...)`. It has nothing to do with per-message redelivery
  timing.
- `SqlReceiveLockContext.ScheduleRedelivery(TimeSpan delay, ...)` implements
  `MessageRedeliveryContext` and schedules a **specific message's** next delivery attempt by
  calling `_clientContext.Unlock(lockId, deliveryId, delay, headers)` with whatever `delay` it is
  given by its caller — it does not read `ReceiveTransportRetryPolicy`, does not consult any
  `IRetryPolicy` implementation, and does not compute the delay itself; the delay is a parameter
  passed in from the message pipeline above it (the standard `UseMessageRetry`/redelivery filter
  stack, which is what actually owns the `IRetryPolicy`-driven delay calculation for the message).

So the transport's delayed redelivery is not bypassing or duplicating `IRetryPolicy` logic — it is
a mechanical "unlock this message after this delay" primitive that the standard retry-policy pipe
filters call into, the same contract point every transport's redelivery exposes. There is no
double-counting or contradictory delay calculation to find in this pair of files; a correct
write-up says so, precisely, rather than reporting a mismatch that doesn't exist or waving the
question away as unanswerable.

## Grading bands

- **correct** — identifies that `ReceiveTransportRetryPolicy` (transport-connection-level) and
  `ScheduleRedelivery`'s `delay` parameter (message-level, caller-supplied) are different retry
  concepts operating at different layers, that `SqlReceiveLockContext` does not itself consult any
  `IRetryPolicy`, and states explicitly that no disagreement exists between the two — or, if a
  lane finds an actual code path where the transport computes/overrides a delay independent of
  the caller-supplied one, correctly cites the specific lines.
- **partial** — correctly identifies the two mechanisms but does not clearly conclude whether they
  can disagree; or conflates `ReceiveTransportRetryPolicy` with per-message redelivery without
  otherwise getting the mechanism wrong.
- **wrong** — claims the SQL transport implements or overrides `IRetryPolicy` for message
  redelivery, or invents a disagreement not present in the code, or investigates the wrong files.
