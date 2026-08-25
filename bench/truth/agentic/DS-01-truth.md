# DS-01 — ground truth

**Corpus:** csharp (MassTransit) @ 855cf1752c94ca9498e0c45ce8d09fdc9e957dd6 (verified: paths
below exist at this sha, read directly from the local clone under `bench/corpora/csharp`).

## Truth files

- `src/MassTransit/Courier/ExecuteActivityHost.cs`
- `src/MassTransit/Courier/CompensateActivityHost.cs`

## What a correct investigation must identify

Both hosts wrap the pipe send in a nested try/catch with the same outer structure:

```
catch (Exception exception) when ((exception is OperationCanceledException
                                    || exception.GetBaseException() is OperationCanceledException)
                                   && !context.CancellationToken.IsCancellationRequested)
{
    ... NotifyFaulted, add exception events ...
    throw new ConsumerCanceledException(...);
}
```

This outer clause is **byte-for-byte symmetric** between the two hosts (same predicate, same
guard on `context.CancellationToken`, same `ConsumerCanceledException` translation). A correct
answer should recognize this and not report it as a divergence.

The real asymmetry is in the **inner** catch, around the pipe send (`_executePipe.Send` /
`_compensatePipe.Send`), which is what actually catches an `OperationCanceledException` thrown by
the activity's own logic — it never reaches the outer clause, because the inner catch is
unconditional (`catch (Exception exception)`) and swallows it first:

- `ExecuteActivityHost.Send()`: the inner catch guards against clobbering a result the activity
  already set —
  `if (executeContext.Result == null || !executeContext.Result.IsFaulted(out var faultException)
  || faultException != exception) executeContext.Result = executeContext.Faulted(exception);`
  — before evaluating.
- `CompensateActivityHost.Send()`: the inner catch has **no equivalent guard**. It unconditionally
  does `await compensateContext.Failed(exception).Evaluate()`, even if `compensateContext.Result`
  was already set by the activity to something else.

Both hosts continue past their inner catch to `NotifyConsumed` and `next.Send(context)` — i.e. an
`OperationCanceledException` from the activity body is absorbed as a "failed/faulted" result and
the routing slip proceeds, in both directions; it does **not** trigger the outer
`ConsumerCanceledException` translation on either side. That symmetric absorption is itself worth
flagging (a canceled activity is converted to a domain fault, not a cancellation, unless the
cancellation happens outside the inner try) — but it is identical in both hosts, so it is not a
divergence between them.

## Grading bands

- **correct** — identifies that the outer OCE-to-`ConsumerCanceledException` handling is
  symmetric, and identifies the missing result-clobber guard in `CompensateActivityHost` (present
  in `ExecuteActivityHost`, absent in `CompensateActivityHost`) as the actual divergence; a patch
  either adds the guard to `CompensateActivityHost` or documents why its absence is intentional.
- **partial** — correctly identifies that a divergence exists somewhere in the inner catch
  handling, without pinning down the specific missing guard; or finds the real divergence but
  misreports the outer clause as also asymmetric.
- **wrong** — reports the hosts as fully symmetric with no divergence, or reports a divergence in
  the outer OCE clause (which is identical), or proposes a fix in the wrong file.
