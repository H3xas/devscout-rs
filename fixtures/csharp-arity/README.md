# csharp-arity fixtures

Each row below is one file `tests/cli_type_arity.rs` draws on. Case notes live
here, never as a narrative header inside the fixture source. Only the files
below are documented here; the directory's other fixtures back tests outside
this ticket's authority.

| File | What it exercises |
| --- | --- |
| `AnthologyBare.cs` | `Volumes.Anthology`, a zero-type-parameter static class declaring `Collate`; sorts before `AnthologyGeneric.cs`, so its own def is the one an arity-blind lookup lands on. |
| `AnthologyGeneric.cs` | `Volumes.Anthology<T>`, a one-type-parameter static class declaring the SAME member name as its bare sibling -- the shared name is what makes a bare/generic qualifier collision observable. |
| `AnthologyConsumers.cs` | A generic qualifier at the matching arity, a bare qualifier at the matching arity, and a two-argument qualifier that names an arity neither sibling declares; the two named tests assert each line binds its own sibling (told apart by `to_file`) or, for the mismatched line, earns no precise edge. |
| `CodexGeneric.cs` | `Volumes.Codex<T>`, the same shared-member-name shape as the Anthology pair, but sorting BEFORE its bare sibling's own file -- the opposite index order. |
| `CodexPlain.cs` | `Volumes.Codex`, the bare sibling; its own file sorts after `CodexGeneric.cs`. |
| `CodexConsumers.cs` | A bare and a one-argument qualifier over the Codex pair; asserts each line binds its own sibling regardless of which file the def index walks first. |
