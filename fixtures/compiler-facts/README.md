# compiler-facts fixtures

Hand-authored candidate compiler-facts artifacts, matching the shape
`compiler-facts import` accepts and the shape the engine's own protocol mode
is expected to produce. Fields are invented, never copied from a real
codebase.

`candidate.json` -- a clean, fully complete candidate:

- `symbols[0]` and `symbols[2]` sit on the same file and line (two same-line
  occurrences: `Widget.Render` and `Gadget.Render` both referenced at
  `Widget.cs:12`).
- `symbols[0]` and `symbols[1]` are two overloads of one name
  (`Widget.Render()` and `Widget.Render(bool)`).
- `symbols[3]` is an unresolved site (`Widget.Bind`, `unresolved: true`).
- `symbols[4]` is a failed binding (`Widget.Load`, `bindingFailed: true`,
  carrying its own diagnostic code).

Every identity field matches the constants the Rust admission path expects
at contract version 1 (`dependencyFingerprint` is the sha256 of
`tools/scout-semantic/packages.lock.json` as of the engine build the
admission path pins; recompute it if that lock file changes).

`candidate-partial.json` -- one unit (`Api|net9.0`) fails to bind while its
sibling (`Shared|net9.0`) is retained in full: `coverage.state` is
`incomplete`, `coverage.incompleteUnits` names the failing unit and its
reason, and both units still appear in `units.processed` (a partial unit is
still processed, just diagnostically incomplete -- it is never moved to
`units.missing`).

Every mismatch/refusal test mutates a copy of `candidate.json` in place
rather than adding a fixture file per case, the same way
`fixtures/import-edges/export.json`'s neighbours do for their own malformed
cases.
