# scout-semantic — compiler oracle for `uses-member` edges

A small C# console tool that opens a **compiled** C# solution with Roslyn and emits one
JSONL record per member reference it finds in the source. It is the ground truth that
`devscout audit --semantic` scores devscout's tree-sitter `uses-member` edges against.

It is **not** part of the Rust crate build: `Cargo.toml` excludes `tools/`, and nothing in
`src/` depends on it. Build and run it only when producing or refreshing an oracle snapshot.

## Prerequisites

- .NET SDK 9.0.3xx (`dotnet --version`). The project targets `net9.0` with
  `<RollForward>Major</RollForward>`, so a newer runtime also works.
- The oracle's own packages, restored from the committed lock file:

  ```sh
  dotnet restore tools/scout-semantic --locked-mode
  dotnet build   tools/scout-semantic --no-restore -c Release
  ```

- **The target solution must be restored first.** Roslyn's `MSBuildWorkspace` evaluates the
  target's MSBuild files in an out-of-process build host; it does not restore for you, and an
  unrestored project loads without its metadata references, which silently turns real symbols
  into unresolved candidates.

  ```sh
  dotnet restore <path/to/Target.sln> [-p:Name=Value ...]
  ```

  Pass the same `-p:` properties to `dotnet restore` and to this tool. A solution whose
  projects multi-target a framework newer than the installed SDK needs, for example,
  `-p:TargetFrameworks=net9.0` (fallback `-p:TargetFramework=net9.0`) on both.

## Usage

```
scout-semantic <path.sln|path.csproj> --root <repo-root> --out <refs.jsonl>
    [--units <units.jsonl>] [--defs <defs.jsonl>]
    [--scope dir[,dir]]       walk only documents under these root-relative dirs
    [--projects glob[,glob]]  load/walk only projects whose name matches (`*` and `?`)
    [--tfm net9.0]            variant to keep when Roslyn splits a multi-targeting project
    [-p Name=Value | -p:Name=Value]   MSBuild global property, repeatable
    [--strict]                exit 2 if any project failed to load
```

`--root` is the repository root every emitted path is made relative to; it does not have to be
the solution directory. Progress and workspace diagnostics go to **stderr**; **stdout stays
empty**, so the tool composes in pipelines. Output files' parent directories are created.

Example:

```sh
dotnet run --project tools/scout-semantic --no-build -c Release -- \
    path/to/Target.sln --root path/to/repo \
    --out out/semantic/refs.jsonl --units out/semantic/units.jsonl --strict
```

### Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | usage error, missing input, or an I/O failure writing the output |
| 2 | `--strict` and at least one project failed to load or produced no compilation |
| 3 | zero projects loaded from the given solution or project |

Without `--strict` the tool **fails open**: a project that cannot be loaded is reported on
stderr and recorded with `"status":"failed"` in `units.jsonl`, and the run still succeeds.
Use `--strict` in CI.

## What is walked

Every document of every loaded project, sequentially, except documents that have no file path,
are not under `--root`, are outside `--scope`, or have a path component in devscout's own skip
list: `bin`, `obj`, `node_modules`, `.git`, `.scout`, `dist`, `coverage`, `.next`, `target`.

Three syntax shapes produce records:

| Syntax | `shape` | `receiverKind` |
|---|---|---|
| `a.M`, `a.P` (simple member access) | `access` | `this`, `base`, `ident`, `qualified`, `call`, `other` |
| `?.M` (member binding) | `conditional` | `conditional` |
| `M(...)` with a bare callee | `bare` | `implicit` |

`receiverKind` for `access` follows the syntax of the qualifier: `this` / `base` expressions,
`ident` for an identifier or generic name, `qualified` for a nested member access or qualified
name, `call` for an invocation, `other` for anything else (literals, `new`, parenthesised).

The symbol comes from `GetSymbolInfo`. When it is null and candidates exist, **one record per
candidate** is emitted with `"ambiguous":true`. Only methods (excluding local functions,
constructors, destructors and lambdas), properties, fields and events are kept; results that
are types or namespaces are dropped. A reduced extension method is un-reduced to its
`ReducedFrom` definition and flagged `"ext":true`, so its `target` is the **static class** that
declares it. Every symbol is normalised through `OriginalDefinition` before ids are built.

Lines are 1-based and **unmapped** (`#line` directives are ignored). `startLine` is the line
the whole member-access node starts on — the first token of the qualifier, which is what
devscout records for a `uses-member` edge, and therefore the join key. `line` is the line of
the member's own name token.

## Id rules

Ids reproduce the shape of devscout's def ids:

- A named type is `Namespace.Outer+Inner`: namespace, then the containing-type chain from
  outermost to innermost joined with `+`. Generic **arity is dropped** (a generic type is spelled by its bare name),
  and generic instantiations are reduced to their `OriginalDefinition`.
- A type in the global namespace has no namespace prefix.
- `targetKind` is `class`, `struct`, `interface`, `enum`, `delegate`, or `record` whenever the
  type is a record (`record class` and `record struct` alike).
- An **enum member** is special-cased: `target` is `Namespace.Enum.Member` and both
  `targetKind` and `memberKind` are `enum-member`. Everything else targets its containing type.
- Arrays and pointers append `[]` / `*` to the element id; type parameters and other exotic
  types fall back to their Roslyn display string. These only ever appear in `receiver`.
- `external` is true when the symbol has no declaring syntax (it came from metadata).
- `targetFile` is the root-relative path of the member's first in-tree declaration, else the
  containing type's first in-tree declaration, else `null`.
- `targetUnit` is the loaded project whose `AssemblyName` matches the symbol's assembly, else
  `null`.

## Output

### `refs.jsonl` (`--out`, required)

One JSON object per line, every key always present, `null` when unknown, in this fixed order:

```json
{"file":"src/App/Worker.cs","startLine":21,"line":21,"shape":"access","receiverKind":"ident",
 "receiverText":"_logger","receiver":"Microsoft.Extensions.Logging.ILogger","member":"LogInformation",
 "memberKind":"method","target":"Microsoft.Extensions.Logging.LoggerExtensions","targetKind":"class",
 "targetFile":null,"targetUnit":null,"ext":true,"external":true,"ambiguous":false,"unit":"App"}
```

`memberKind` is one of `method`, `property`, `field`, `event`, `enum-member`. `receiverText` is
the qualifier's source with runs of whitespace collapsed to one space and truncated to 120
characters. Records are sorted by `(file, startLine, line, member, target, ambiguous)` and
de-duplicated on that same tuple — which is what collapses the overload candidates of one
ambiguous site into a single row. Paths always use `/`; numbers are invariant-culture.

### `units.jsonl` (`--units`)

One object per loaded project, sorted by name:

```json
{"name":"App","path":"src/App/App.csproj","tfm":"net9.0","test":false,"status":"ok","diagnostics":0,
 "refs":["Domain","Ext.Adapters"],"files":["src/App/AppDbContext.cs","src/App/Worker.cs"]}
```

`test` is true when the project file mentions `Microsoft.NET.Test.Sdk` or `<IsTestProject>true`.
`diagnostics` counts error-severity compiler diagnostics. `refs` are project-reference names.
`files` are the documents actually walked.

### `defs.jsonl` (`--defs`, optional)

`{"id","kind","file","line","unit","test"}` for every in-tree named type and enum member,
sorted by `(id, file, line)`. `test` is true when the type declares a method carrying one of
devscout's test attributes — `Fact`, `Theory`, `Test`, `TestCase`, `TestCaseSource`, plus
`TestMethod` / `DataTestMethod` only inside a `[TestClass]`, with the `Attribute` suffix
tolerated. A partial type yields one row per declaring file. The audit does not require this
file.

## Multi-targeting

Roslyn splits a multi-targeting project into one `Project` per framework, named `Name(tfm)`.
Projects are grouped by project-file path; `--tfm` selects the variant to keep, and the first
variant is used when nothing matches. The reported `name` has the `(tfm)` suffix stripped and
the framework moves to the `tfm` field.

## Implementation notes and known limits

- Sequential by design: one project, one document, one syntax node at a time. There is no
  parallelism to keep the output byte-identical between runs and machines.
- `MSBuildLocator.RegisterDefaults()` runs before any Roslyn MSBuild type is touched. If it
  finds no instance the tool warns and continues, because Roslyn ≥ 4.9 evaluates projects in an
  out-of-process build host anyway.
- `receiver` for a `conditional` record is the type of the expression the enclosing `?.` chain
  tests; a `bare` record has no receiver expression, so `receiver` and `receiverText` are null.
- `files` in `units.jsonl` honours `--scope`, so it always lists exactly the documents that
  could contribute records.
- `diagnostics` is only computed when `--units` is requested, because it forces a full binding
  pass over the project.
- There is no `--help` flag: an unknown option prints the usage block on stderr and exits 1.
- Source-generated documents are not walked; only files on disk are.

## Packages

Pinned in `packages.lock.json` and restored with `--locked-mode`:

| Package | Version |
|---|---|
| `Microsoft.Build.Locator` | 1.9.1 |
| `Microsoft.CodeAnalysis.CSharp.Workspaces` | 4.14.0 |
| `Microsoft.CodeAnalysis.Workspaces.MSBuild` | 4.14.0 |

4.14.0 is the newest 4.14.x release and the Roslyn line that ships with the 9.0.3xx SDK.
Bumping to a 5.x line requires a matching newer SDK and a lock-file refresh
(`dotnet restore tools/scout-semantic --force-evaluate`).
