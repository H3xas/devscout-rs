# DS-03

**Corpus:** csharp (MassTransit) @ 855cf1752c94ca9498e0c45ce8d09fdc9e957dd6
**Source:** maintainer-authored ticket, permitted for this run under the 2026-08-25 sourcing
waiver in `docs/benchmarks/agentic.md`; not sourced from MassTransit's public issue tracker.

**Statement**

A consumer using the batching feature (`IConsumer<Batch<T>>`) behaves differently under retry
than a plain single-message handler consuming the same message type — messages that come back in
as a retry of a previously failed delivery don't seem to wait for the batch to fill up the normal
way, and it's unclear whether a batch-level failure correctly counts against each individual
message's retry state the way a single-message handler's failure would.

Compare how retries and exception handling are wired for the batching consumer path against how a
plain message handler is wired, identify where the two diverge, and document whether each
divergence you find is required by how batching works or is a gap that should be closed.

**Definition of done:** a written comparison identifying every point of divergence, each one
classified as required-by-design or gap, and a patch for any divergence classified as a gap.
