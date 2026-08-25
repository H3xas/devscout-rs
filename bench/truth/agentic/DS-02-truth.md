# DS-02 — ground truth

**Corpus:** csharp (MassTransit) @ 855cf1752c94ca9498e0c45ce8d09fdc9e957dd6 (verified: paths
below exist at this sha, read directly from the local clone under `bench/corpora/csharp`).

## Pattern to follow (the trio, per existing policy)

Every built-in retry policy is three files in `src/MassTransit/RetryPolicies/`:

- `<Name>RetryPolicy.cs` — implements `IRetryPolicy` (`CreatePolicyContext<T>`, `IsHandled`,
  `Probe`), e.g. `ImmediateRetryPolicy.cs`, `IncrementalRetryPolicy.cs`.
- `<Name>RetryContext.cs` — implements `RetryContext<TContext>` via `BaseRetryContext<TContext>`,
  computing whether/how to retry (`CanRetry`), e.g. `ImmediateRetryContext.cs`.
- `<Name>RetryPolicyContext.cs` — extends `BaseRetryPolicyContext<TContext>`, creating the
  `RetryContext` for attempt 0, e.g. `ImmediateRetryPolicyContext.cs`.

`MessageRetryPolicyExtensions.cs` is the consumer of the trio (`Retry<T>`/`Attempt<T>`), not a
file to modify for this ticket — it calls `IRetryPolicy` generically and needs no change for a
new policy to work.

## Load-bearing context: jitter already exists, just not as a standalone configurable policy

`src/MassTransit/RetryPolicies/ExponentialRetryPolicy.cs` already applies jitter internally:
`CalculateIntervals()` uses `random.Next(_lowInterval, _highInterval)` per step, and
`GetRetryInterval(int)` additionally multiplies by `new Random().NextDouble() * 0.5 + 0.75`. That
jitter is **not configurable** (no min/max jitter fraction parameter) and is bound to the specific
delta-based interval calculation `ExponentialRetryPolicy` already does. A lane that reports "there
is no jitter, I added it" without noticing existing jitter in `ExponentialRetryPolicy` has missed
load-bearing context, even if the new policy it writes is otherwise reasonable. The ticket asks
for a *new* policy (explicitly configurable min/max/jitter), not a fix to the existing one.

## Wiring point beyond the trio

- `src/MassTransit/Configuration/RetryConfigurationExtensions.cs` and `src/MassTransit/Retry.cs`
  are where `.Immediate(...)`, `.Incremental(...)`, `.Exponential(...)`-style builder methods live
  on the public retry configuration surface. A complete answer adds an equivalent builder method
  here for the new policy — the ticket's "wire it into the public retry configuration surface"
  line points here.

## Grading bands

- **correct** — three new files following the trio pattern, `IRetryPolicy` implemented correctly,
  a builder method added to the public configuration surface, delays that grow with attempt count
  and vary within a configurable jitter bound, with a test.
- **partial** — the trio is implemented but not wired into the public configuration surface (only
  usable via direct instantiation), or jitter/backoff math has a real defect (e.g. jitter always
  the same sign, or no exponential growth), or no test.
- **wrong** — no new policy, or the existing `ExponentialRetryPolicy` is modified in place and
  presented as satisfying "add a new policy," or the trio structure is not followed at all.
