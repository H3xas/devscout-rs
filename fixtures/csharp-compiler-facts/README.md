# csharp-compiler-facts fixture

A package-free multi-file fixture for the engine's `--emit compiler-facts` protocol mode.
`compiler-facts.json` is the committed snapshot: `--emit compiler-facts` run against
`Fixture.csproj` with `--tfm net9.0 -p Configuration=Debug -p Platform=AnyCPU --no-git` and no
`--capabilities` flag (the engine's own default, which now includes `occurrences`), reproduced by
CI's `semantic-audit` job and diffed byte-for-byte, the same way `fixtures/csharp-flowtrace/` pairs
a fixture solution with its own committed fact document.

`Widgets.cs` is unchanged from before per-reference occurrence facts existed, and keeps exercising
the declared-symbol/diagnostic facts the protocol document must be able to carry:

- `Widget.Render()` and `Widget.Render(bool)` sit on the same source line -- two same-line
  occurrences and two overloads of one name at once.
- `Widget.Load()` calls a method that does not exist (`CS1061`) -- a failed binding, which demotes
  the whole unit's `coverage.state` to `incomplete` with the compiler's own reason.
- `Widget.Reference()` references an undeclared identifier (`CS0103`) -- an unresolved site,
  carried as a second diagnostic, and (as `occurrences.sites`) the unresolved occurrence AC-11's own
  control case needs: the oracle's own `refs.jsonl` would have dropped this site; this producer does
  not.
- `Gadget` is a second, unrelated type in the same file, proving a qualified unit's other facts are
  retained rather than discarded wholesale alongside the failing ones.

`Callers.cs` and `Other.cs` are new, added to exercise per-reference occurrence facts specifically,
each shape kept in exactly one deliberate call:

- `widget.Render(); widget.Render(true);` (same line as each other) -- two occurrences of one
  target on one line, two overloads bound differently at each site, both `confirmed`.
- `helper.Assist()` -- a call whose target (`Helper`, in `Other.cs`) is declared in a document other
  than the caller's own, so the occurrence's `targetDocumentContentIdentities` names a document
  distinct from its own `file`.
- `helper.Secret()` -- a private member referenced from outside its declaring type: `inaccessible`,
  one candidate, `candidateReason: "Inaccessible"`.
- `widget.Render("mismatched")` -- a string argument neither `Render()` nor `Render(bool)` accepts:
  `ambiguous`, two candidates, `candidateReason: "OverloadResolutionFailure"`.
- `widget.Render(dynamicArgument)` -- a `dynamic`-typed argument against a target with exactly one
  arity-compatible overload: still resolved at compile time (`confirmed`), `candidateReason` still
  reports `"LateBound"` for the informational record even though a symbol was found.
- `helper.Choose(dynamicArgument)` -- a `dynamic`-typed argument against a target with two
  arity-compatible overloads (`Choose(int)`/`Choose(string)`, declared only for this case): no
  symbol can be chosen at compile time at all, `dynamic`, two candidates,
  `candidateReason: "LateBound"`.
- `Callers.Nested<T>.Go()` calls `Helper.Assist()` -- a call inside a generic, nested caller, so the
  occurrence's `caller` identity carries a nested `type` id (`Outer+Inner`).

Two runs over these pinned inputs are byte-identical (no timestamp, no path outside the fixture,
`--no-git` so no source-snapshot identity is stamped). `dependencyFingerprint` is the sha256 of
`tools/scout-semantic/packages.lock.json`. `context.envelope` is the real embedded build-context
envelope (one record per compilation, the same shape `--emit context` itself writes);
`context.contextFingerprint` is the derived summary folded from that envelope's own per-compilation
`identity`/`fingerprint` pairs -- the Rust admission path recomputes it the same way rather than
comparing two producer-written copies of one value, and reads nothing else from the envelope body.

This snapshot's diff from the pre-occurrence-facts snapshot is exactly: the new
`Callers.cs`/`Other.cs`-derived `symbols`/`diagnostics` growth, the new top-level `occurrences` key,
the real embedded `context.envelope` (replacing a frozen placeholder), and the two header literals
`artifactSchemaVersion`/`producer.engineRevision` this delta deliberately moves -- reviewed as part
of this change, not assumed clean by precedent.

Regenerate with (from the repository root, after `dotnet build tools/scout-semantic -c Release`):

```sh
dotnet run --project tools/scout-semantic --no-build -c Release -- \
  fixtures/csharp-compiler-facts/Fixture.csproj --root fixtures/csharp-compiler-facts \
  --emit compiler-facts --compiler-facts fixtures/csharp-compiler-facts/compiler-facts.json \
  --tfm net9.0 -p Configuration=Debug -p Platform=AnyCPU --no-git
```

A run restricted to the pre-existing capabilities (no `occurrences` key, otherwise byte-identical)
is produced the same way, adding `--capabilities symbols,diagnostics`:

```sh
dotnet run --project tools/scout-semantic --no-build -c Release -- \
  fixtures/csharp-compiler-facts/Fixture.csproj --root fixtures/csharp-compiler-facts \
  --emit compiler-facts --compiler-facts /tmp/compiler-facts-restricted.json \
  --tfm net9.0 -p Configuration=Debug -p Platform=AnyCPU --capabilities symbols,diagnostics --no-git
```
