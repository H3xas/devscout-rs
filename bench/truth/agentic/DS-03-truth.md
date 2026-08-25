# DS-03 — ground truth

**Corpus:** csharp (MassTransit) @ 855cf1752c94ca9498e0c45ce8d09fdc9e957dd6 (verified: paths
below exist at this sha, read directly from the local clone under `bench/corpora/csharp`).

## Truth files

- `src/MassTransit/Consumers/HandlerExtensions.cs`
- `src/MassTransit/Consumers/Batching/BatchConsumer.cs`
- `src/MassTransit/Consumers/Batching/BatchConsumerFactory.cs`

## What a correct comparison must identify

`HandlerExtensions.cs` contains **no retry- or exception-wiring code at all** — it is pure
connector plumbing (`Handler<T>`, `ConnectHandler<T>`, `ConnectRequestHandler<T>`), delegating to
`HandlerConfigurator<T>` / `HandlerConnectorCache<T>`. For a plain single-message handler,
exception filtering and retry are entirely the standard receive-endpoint pipe filters
(`UseMessageRetry`, configured via `RetryConfigurationExtensions.cs` /
`MessageRetryConfigurationExtensions.cs`), external to the handler-connection code itself. This
absence is the expected baseline, not a gap — a single message has no cross-message retry state to
propagate, so there is nothing analogous for `HandlerExtensions.cs` to do.

`BatchConsumer.cs` (`Consumers/Batching/`) has batching-specific retry logic that has no
single-message analogue:

- `IsReadyToDeliver(ConsumeContext context)` (line ~148): `if (context.GetRetryAttempt() > 0)
  return true;` — a message that is itself a retry/redelivery forces immediate batch delivery
  rather than waiting for the batch to fill to `_options.MessageLimit`. This is the "retried
  messages don't wait for the batch to fill" behaviour named in the ticket, and it is intentional:
  holding a redelivered message hostage waiting for unrelated messages to arrive would extend its
  retry latency for no reason.
- `Deliver(...)` (line ~166): on a non-cancellation exception from `_consumerPipe.Send(...)`, if
  `batchConsumeContext.TryGetPayload(out RetryContext<ConsumeContext<Batch<TMessage>>>
  retryContext)` finds a retry context left by an upstream `UseMessageRetry` filter on the batch
  pipe, it is propagated onto **every individual message in the batch**
  (`messages[i].GetOrAddPayload(() => retryContext)`), so a batch-level failure's retry state is
  visible to each constituent message, not just the batch as a whole.

`BatchConsumerFactory.cs` has no retry-specific logic of its own; `Send<T>()` collects into a
`BatchConsumer` and forwards to `next.Send(...)`, with completion signaled via
`_collector.Complete(...)` in a `finally` — retry behaviour for the batch send outcome is handled
entirely by the pipe filters wrapping `next`, the same as for a single-message consumer.

## Grading bands

- **correct** — identifies both of the `BatchConsumer` mechanisms above (early delivery on
  `GetRetryAttempt() > 0`, and retry-context propagation to every message in `Deliver`), states
  that `HandlerExtensions.cs` has no equivalent because retry for single messages is handled by
  the standard pipe filters rather than by the handler-connection code, and classifies both
  `BatchConsumer` mechanisms as required-by-design rather than bugs (or gives a specific,
  code-grounded reason why one should change).
- **partial** — identifies one of the two `BatchConsumer` mechanisms but not the other, or
  correctly identifies the mechanisms but misclassifies them as bugs without a concrete
  justification grounded in the code.
- **wrong** — reports `HandlerExtensions.cs` as missing retry wiring that should be added there,
  or misses both `BatchConsumer` mechanisms.
