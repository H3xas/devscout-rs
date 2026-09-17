# csharp-compiler-facts fixture

A package-free single-project fixture for the engine's `--emit compiler-facts` protocol mode.
`compiler-facts.json` is the committed snapshot: `--emit compiler-facts` run against
`Fixture.csproj` with `--tfm net9.0 -p Configuration=Debug -p Platform=AnyCPU --no-git`, reproduced
by CI's `semantic-audit` job and diffed byte-for-byte, the same way `fixtures/csharp-flowtrace/`
pairs a fixture solution with its own committed fact document.

`Widgets.cs` is deliberately shaped to exercise the facts the protocol document must be able to
carry:

- `Widget.Render()` and `Widget.Render(bool)` sit on the same source line -- two same-line
  occurrences and two overloads of one name at once.
- `Widget.Load()` calls a method that does not exist (`CS1061`) -- a failed binding, which demotes
  the whole unit's `coverage.state` to `incomplete` with the compiler's own reason.
- `Widget.Reference()` references an undeclared identifier (`CS0103`) -- an unresolved site,
  carried as a second diagnostic.
- `Gadget` is a second, unrelated type in the same file, proving a qualified unit's other facts are
  retained rather than discarded wholesale alongside the failing ones.

Two runs over these pinned inputs are byte-identical (no timestamp, no path outside the fixture,
`--no-git` so no source-snapshot identity is stamped). `dependencyFingerprint` is the sha256 of
`tools/scout-semantic/packages.lock.json`; `context.contextFingerprint` and
`context.envelope.fingerprint` are a frozen placeholder pending a real per-compilation context
producer -- the Rust admission path checks only the embedded envelope's version literal, never its
internal shape, so this fixture needs to move only if that placeholder's shape itself changes.

Regenerate with (from the repository root, after `dotnet build tools/scout-semantic -c Release`):

```sh
dotnet run --project tools/scout-semantic --no-build -c Release -- \
  fixtures/csharp-compiler-facts/Fixture.csproj --root fixtures/csharp-compiler-facts \
  --emit compiler-facts --compiler-facts fixtures/csharp-compiler-facts/compiler-facts.json \
  --tfm net9.0 -p Configuration=Debug -p Platform=AnyCPU --capabilities symbols,diagnostics --no-git
```
