# csharp-truth fixtures

The semantic-truth harness's own case manifest and fixture pack. Every case in
`manifest.json` is independently authored against the source under `src/`; no
case's expectation is generated from this repository's own analyzer output.
Fixture vocabulary is invented, never copied from a real codebase.

| Case | File(s) | What it exercises |
| --- | --- | --- |
| `overload-arity-a` | `src/Overloads.cs` | Two distinct overloads called on one line stay distinct occurrences and distinct identities. |
| `generic-arity-a` | `src/GenericArity.cs` | A one-type-argument and a two-type-argument overload of the same name are distinct identities. |
| `implementation-identity-a` | `src/ImplementationIdentity.cs` | A call through a class-typed receiver binds the class's own declaration, never the interface it implements. |
| `failed-binding-healthy-unit-a` | `src/FailedBinding.cs` | A call through an undeclared receiver (`MissingApi`) must stay unresolved even though the owning unit otherwise reports healthy. |
| `dropped-project-a` | `src/DroppedProject/ProjectA/A.cs`, `src/DroppedProject/ProjectB/B.cs` | A cross-project call resolves only when both requested projects are present; the fault-control battery's dropped-project row exercises what happens when one is not. |
| `framework-semantics-a` | `src/FrameworkSemantics.cs` | A DI-registration-shaped call is a positive framework-semantics fact, and an unassigned-local-variable use is an expected compiler diagnostic on the identifier itself. |

Fixtures with no manifest case of their own -- committed as red-baseline
evidence and freshness-pair inputs instead, per `red-baseline.json`:

| Fixture | What it evidences |
| --- | --- |
| `src/UnrelatedApiNames.cs` | The `unrelated-names-promoted-to-framework-facts` red-baseline row: ordinary unrelated methods whose names happen to match a framework convention. |
| `src/NativeDispatchCounterexample.cs` | The `type-argument-pair-produces-implements-edges-on-a-clean-build` red-baseline row. |
| `src/DirectoryMove/Before/Packet.cs`, `src/DirectoryMove/After/Messaging/Messages/Packet.cs` | A layout-only move (same class, same bytes, new directory) -- the `directory-move-creates-message-class-fact` red-baseline row and a freshness-pair input: the class's own identity must be preserved across the move even though a convention-based producer elsewhere is not. |

Every case's `profiles` names `csharp-net8.0-sdk`, the one profile this
fixture pack's fast lane exercises; the profile registry in
`src/truth/profile.rs` registers the wider intended inventory separately.

## `reports/`

`reports/syntax-only.json` and `reports/enriched.json` are the two persisted
lane artifacts a consumer reads offline, alongside `manifest.json` and
`capability-matrix.json`. Neither is graded from a real producer's output
today: `reports/syntax-only.json`'s observations are the manifest's own
`present`-fact expectations, and its `pinnedInputs.corpusRevision` says so;
`reports/enriched.json` is the explicitly marked synthetic stand-in pending
a real enriched producer. Regenerate both with
`cargo test --test semantic_truth_determinism` and re-commit them whenever
`manifest.json` or a file under `src/` changes -- the byte-identity test
fails otherwise, and the committed pin (recorded in each report's header)
covers both.
