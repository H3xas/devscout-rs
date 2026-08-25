# DS-02

**Corpus:** csharp (MassTransit) @ 855cf1752c94ca9498e0c45ce8d09fdc9e957dd6
**Source:** maintainer-authored ticket, permitted for this run under the 2026-08-25 sourcing
waiver in `docs/benchmarks/agentic.md`; not sourced from MassTransit's public issue tracker.

**Statement**

We want a new built-in retry policy — call it whatever fits the existing naming — that retries on
a jittered exponential backoff: each attempt roughly doubles the prior delay, with a
caller-configurable minimum and maximum interval, and randomized jitter on each computed delay so
that many consumers retrying the same failure don't all hammer the broker in lockstep.

It needs to be usable anywhere the built-in retry policies are (e.g. wherever `.Immediate(...)`
or `.Incremental(...)` can be configured today), follow the same Policy / RetryContext /
RetryPolicyContext structure the existing policies use so it drops into the pipeline the same
way, and probe its configuration the same way the others do.

Implement it, following the existing pattern as closely as makes sense, and wire it into the
public retry configuration surface.

**Definition of done:** a new retry policy implementing the standard trio, a builder method on
the public retry configuration surface that constructs it, and a test proving the computed delays
grow roughly exponentially and vary run to run within the configured jitter bound.
