# call-site-evidence fixture

A single, package-free, independently authored C# file (`Witness.cs`) used to prove the call-site
evidence contract: which of devscout's native `--json` answers and the optional `flowtrace-facts`
sidecar preserve one invocation as its own distinct site, and which collapse two into one.

`EXPECTED.md` records every witness's expected call site, authored by reading `Witness.cs` alone,
before any `devscout` command is run against it -- `tests/call_site_evidence.rs` checks the
audited surfaces against that list, never the other way around. `docs/call-site-evidence-matrix.md`
is regenerated from real command output against this fixture (see that test's own doc comment),
never hand-transcribed.

| Method | Witness shape |
| --- | --- |
| `Caller.RepeatedCallsDifferentLines` | Repeated calls to one target, on two different lines. |
| `Caller.TwoCallsOneLine` | Two calls to one target on the same line -- the one genuine row-collision this fixture demonstrates. |
| `Caller.OverloadAmbiguity` | Two calls naming the same member, binding different overloads by argument shape. |
| `Caller.Recurse` | A call site inside the callee's own body, `this.`-qualified (see `EXPECTED.md` for why an unqualified self-call is a separate, recorded gap rather than a second case here). |
| `Caller.AwaitedSequence` | Two awaited calls to the same async target, one after another. |
| `Caller.ParallelLaunchJoin` | Two calls launched before either is awaited, then joined together. |

No employer source, path, snapshot or internal identifier appears anywhere in this fixture; every
name is invented for this repository.
