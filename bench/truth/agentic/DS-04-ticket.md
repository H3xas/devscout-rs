# DS-04

**Corpus:** csharp (MassTransit) @ 855cf1752c94ca9498e0c45ce8d09fdc9e957dd6
**Source:** maintainer-authored ticket, permitted for this run under the 2026-08-25 sourcing
waiver in `docs/benchmarks/agentic.md`; not sourced from MassTransit's public issue tracker.

**Statement**

The SQL transport supports delayed message redelivery — a message can come back for another
attempt after a broker-native delay instead of an immediate retry. It's not clear whether that
redelivery delay is coordinated with the message-level retry policy an endpoint has configured
(`IRetryPolicy` — the same contract `ImmediateRetryPolicy`, `IncrementalRetryPolicy` and friends
implement), or whether the transport is scheduling its own delay independent of whatever policy
the consumer's pipeline is using.

Investigate how the SQL transport's delayed redelivery relates to the standard `IRetryPolicy`
contract, note any place the two could disagree about when or how many times a message should
come back, and write up what you find.

**Definition of done:** a written explanation of how (or whether) the transport's redelivery
delay is derived from or coordinated with the configured `IRetryPolicy`, and a patch for any
disagreement found that could cause incorrect retry counting or timing.
