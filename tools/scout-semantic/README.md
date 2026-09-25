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

- **A never-restored target is restored automatically, offline.** Roslyn's `MSBuildWorkspace`
  evaluates the target's MSBuild files in an out-of-process build host; it does not restore for
  you, and an unrestored project loads without its `PackageReference`-resolved metadata
  references, which silently turns real symbols into unresolved candidates. Before the workspace
  is used for facts, the tool therefore checks every project the workspace opened (including a
  project reference that target selection left out) for its assets file, at the location MSBuild
  evaluates for it, so a custom intermediate path is honoured. A project the tool's in-process
  MSBuild fails to evaluate for any reason (a `netstandard2.0` project calling an intrinsic it
  does not implement, or a registered MSBuild whose assemblies cannot bind in process) is
  evaluated by the installed SDK's own `dotnet msbuild -getProperty:ProjectAssetsFile` with the
  run's `-p:` properties.
  Each project whose assets file is missing is restored alone, without its project-reference
  closure, from the local NuGet global packages folder only -- the folder NuGet resolves for the
  analysed root (`NUGET_PACKAGES`, a `globalPackagesFolder` setting, or the default
  `~/.nuget/packages`). The restore passes that folder as `--source` and pins
  `RestoreSources` to it and `RestoreAdditionalProjectSources` to empty after every forwarded
  `-p:` property, so neither a configured feed, a project's own additional source, nor a forwarded
  source property is consulted, and the restore never reaches the network. A package that folder
  cannot satisfy stays unresolved rather than triggering a wider search. A project whose assets
  file already exists is never restored again (the file's bytes and timestamp are unchanged). The
  restore writes wherever MSBuild places its output, normally the git-ignored `obj/` directory --
  the same side effect an ordinary `dotnet restore` has, performed by this tool instead of by
  hand.

  `--no-restore` skips this step entirely and reproduces the tool's pre-restore behaviour: an
  unrestored project's `PackageReference` types report `CS0234`/`CS0246` exactly as before. A
  project the restore could not complete -- the package is not in the local cache, or the restore
  otherwise failed -- is reported with build-context state `partial` and reason `unrestored`,
  ahead of the generic `binding-error` reason its compiler diagnostics would otherwise produce,
  and in `--emit compiler-facts` its `coverage.incompleteUnits` entry names the restore failure
  instead of the first compiler error; see [Build-context envelope](#build-context-envelope) and
  [Compiler-facts protocol](#compiler-facts-protocol) below. A failed restore still writes an
  assets file that records the error in its `logs`; later runs read that record, keep reporting
  the project `unrestored` with the recorded error, and do not restore it again. Delete the
  project's assets file (or restore it by hand) once the package is available locally.

  A manual restore ahead of time still works exactly as before and is a no-op for this tool (its
  own check finds the assets file already there):

  ```sh
  dotnet restore <path/to/Target.sln> [-p:Name=Value ...]
  ```

  Pass the same `-p:` properties to `dotnet restore` and to this tool. A solution whose
  projects multi-target a framework newer than the installed SDK needs, for example,
  `-p:TargetFrameworks=net9.0` (fallback `-p:TargetFramework=net9.0`) on both -- the offline
  restore step forwards this tool's own `-p:` properties to its `dotnet restore` invocation the
  same way.

## Usage

```
scout-semantic <path.sln|path.csproj> --root <repo-root> --out <refs.jsonl>
    [--emit mode[,mode]]      oracle | flowtrace-facts | compiler-facts | context, repeatable (default: oracle)
    [--units <units.jsonl>] [--defs <defs.jsonl>]
    [--facts <path|->]        fact document, default out/facts/<repo>.json, `-` is stdout
    [--compiler-facts <path|->]  compiler-facts protocol document, `-` is stdout
    [--capabilities a,b]      requested capability names, repeatable, compiler-facts only
    [--context <path|->]      build-context envelope, required with `--emit context`, `-` is stdout
    [--repo <id>]             repo id in the fact/context header (default: --root's last segment)
    [--no-git]                do not stamp git identity in the fact header
    [--no-restore]            skip the automatic offline restore of any project missing its
                               assets file
    [--publish-calls a,b]     extra publish method names, repeatable
    [--consumer-bases a,b]    extra consumer base type names, repeatable
    [--scope dir[,dir]]       walk only documents under these root-relative dirs
    [--projects glob[,glob]]  load/walk only projects whose name matches (`*` and `?`)
    [--tfm net9.0]            requested target, repeatable; each is its own compilation identity
    [-p Name=Value | -p:Name=Value]   MSBuild global property, repeatable
    [--strict]                exit 2 on a failed project, an unresolved fact site, or (with
                               `--emit context`) a context artifact that is not complete
```

`--root` is the repository root every emitted path is made relative to; it does not have to be
the solution directory. Progress and workspace diagnostics go to **stderr**; **stdout stays
empty** unless `--facts -`/`--compiler-facts -`/`--context -` asks for a document there, so the
tool composes in pipelines. Output files' parent directories are created.

`--emit` selects the output modes. `oracle` is the refs/units/defs output described below and is
what runs when the flag is absent; `flowtrace-facts` is the [fact document](#flow-tracer-facts);
`compiler-facts` is the [compiler-facts protocol document](#compiler-facts-protocol); `context` is
the [build-context envelope](#build-context-envelope). An unknown mode is a usage error. `--out`
is required only when `oracle` is among the modes, and `--units` / `--defs` are only meaningful
with it; `--compiler-facts` is required only when `compiler-facts` is among the modes, `--context`
only when `context` is. The modes are additive, so
`--emit oracle,flowtrace-facts,compiler-facts,context` writes all four from one load.
`compiler-facts` reuses the existing `--tfm`/`-p Name=Value` machinery for its requested target,
configuration and platform rather than adding dedicated flags for what that surface already
carries.

`--tfm` is repeatable: each requested target is its own compilation identity, and a target a
project does not declare is never silently substituted for another one (see
[Multi-targeting](#multi-targeting)).

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
| 2 | `--strict` and at least one project failed to load, produced no compilation, left a fact site unresolved, or (with `--emit context`) the context artifact's rollup state is not `complete` |
| 3 | zero projects loaded from the given solution or project, and (with `--emit context`) nothing to report even as an `unsupported` or `excluded` record |

Without `--strict` the tool **fails open**: a project that cannot be loaded is reported on
stderr and recorded with `"status":"failed"` in `units.jsonl`, and the run still succeeds.
Use `--strict` in CI. `--strict`'s meaning tightened for `--emit context`: previously every
existing fixture already produced a clean run, so this repository's own CI stays green under the
tightened rule, but any other input shaped like the `net472` binding-error control below would now
see `--strict` exit 2 where it previously exited 0 -- a fix to what "strict" means for a context
artifact, not a silent behavior change.

A fact or context record that does not satisfy its schema is exit 1 with
`error: fact schema violation: <reason>` or `error: context schema violation: <reason>` on
stderr; each document is validated in full before anything is written, so a violation never
leaves a partial file behind.

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

## Flow-tracer facts

`--emit flowtrace-facts` writes **one** JSON document — the fact set the flow tracer consumes as
a provider document — alongside (`--emit oracle,flowtrace-facts`) or instead of the
per-reference JSONL above. The path is `--facts`, which defaults to `out/facts/<repo>.json` and
accepts `-` for stdout. The required-field table the document is checked against mirrors the
flow tracer's own fact schema (its `docs/fact-schema.md`); the source commit is noted in
`FactSchema.cs`.

```sh
dotnet run --project tools/scout-semantic --no-build -c Release -- \
    fixtures/csharp-flowtrace/Fixture.sln --root fixtures/csharp-flowtrace \
    --emit flowtrace-facts --facts out/facts/flowtrace.json --no-git --strict \
    --publish-calls SubmitJob
```

### Header

Header keys come first, in a fixed order, and `facts` is always last:

```json
{
  "schemaVersion": 1,
  "producer": "scout-semantic",
  "version": "0.1.0",
  "repo": "csharp-flowtrace",
  "kind": "backend",
  "generatedFrom": "scout-semantic 0.1.0",
  "compilation": {
    "solution": "Fixture.sln",
    "units": ["Api|net9.0", "Shared|net9.0"],
    "digest": "e18f1cdf4d76ce62e214c1050d8f2ad56bd8f3cd"
  },
  "headSha": "…", "dirty": false, "dirtyDigest": "…", "fileCount": 20,
  "facts": []
}
```

- `version` is the assembly's informational version, pinned in the project file so it carries no
  `+<commit sha>` suffix.
- `repo` is `--repo`, else the last segment of the resolved `--root`.
- `compilation.solution` is the positional path made root-relative with forward slashes, falling
  back to its file name. `units` is one `Name|tfm` entry per loaded project (`?` when the
  framework is unknown), ordinal-sorted, and `digest` is the lower-case hex SHA-1 of those
  entries joined with newlines.
- The four git keys are read with the working directory set to `--root`: `headSha` from
  `git rev-parse HEAD`, `dirty` and `dirtyDigest` from the non-empty lines of
  `git status --porcelain` (sorted ordinal, joined with newlines, SHA-1'd — the digest of the
  empty string when the tree is clean), `fileCount` from the non-empty lines of `git ls-files`.
  All four are omitted together under `--no-git`, when `git` is not on PATH, or when the tree has
  no HEAD; a missing git never fails the run.
- There is no `generatedAt` key, and no fact carries a `provenance` key: the consumer refuses
  facts that declare their own provenance.

### Fact kinds

Every fact carries `type`, `file` (repository-relative, forward slashes) and `line` (1-based)
first, then its required fields in schema order, then its optional fields. Framework shapes are
recognised by **simple name and arity only, never by namespace**, so a repository that declares
its own stand-in types is matched the same way a referenced package is.

| Kind | How it is derived |
|---|---|
| `message_class` | A class or record declared under a `Messaging/Messages` path, or named `*Message`, or implementing `ICorrelatedMessage` / `IMessage` / `CorrelatedBy`; `line` is the body's opening brace, else the identifier. |
| `consume` | A non-abstract class or record whose base chain or interfaces include a one-type-argument `IConsumer` / `BaseConsumer` (plus `--consumer-bases`); a `Batch<T>` argument is unwrapped to `T`. One fact per distinct message. |
| `publish` | A call to `Publish` / `PublishAsync` (plus each `--publish-calls` name and that name with an `Async` suffix). The message is the explicit type argument when there is one, else the first argument's own type, with `await` unwrapped. |
| `ctor_field` | Primary-constructor parameters; constructor-body assignments of a parameter to a field or property (including `this.x = y` and the `_x = x ?? throw …` guard); and the non-framework parameters of a minimal-API handler lambda. |
| `di_binding` | `AddScoped` / `AddTransient` / `AddSingleton` / `Register` with two type arguments, or with one type argument and a factory lambda whose result type supplies the implementation; plus every non-abstract class implementing an `IRequestHandler` / `ICommandHandler` / `IQueryHandler` interface, whose first type argument is the bound request. |
| `iface_impl` | Each interface **written** in a class or record's own base list, abstract types included; the declaration syntax is resolved rather than the symbol, so compiler-synthesised interfaces — a positional record's `IEquatable<T>` — are not facts. |
| `route` | Attribute routing: `[HttpGet]` … `[HttpDelete]` and `[Route]` on a method, combined with the first class-level `[Route]`, with `[controller]` and `[action]` expanded; a method-level `[Route]` with no verb attribute is `ANY`. Minimal API: `MapGet` … `MapDelete` and `MapMethods` with a constant pattern, prefixed by the `MapGroup` chain the receiver resolves through (depth-capped at 8). |
| `method_span` | Every method and constructor with a body inside a named type, plus a minimal-API handler lambda with no enclosing method or constructor (once, whatever its verb count) — a lambda registered from inside a member is already covered by that member's span, so it gets none of its own. |

A handler lambda with no enclosing member takes its `action` — and so its `method_span` method
name — from the `Map*` call itself, following the regex pass's own convention, so several
top-level lambdas registered with the same verb share the name `MapGet`, `MapPost` and so on;
`line` is what tells them apart.

`message_class`, `consume` and the handler-interface `di_binding` describe the **type**, so a
partial type emits them once, at the first part that declares a base list (ties broken by file
path then position). `iface_impl` and `ctor_field` describe the part they are written in and are
emitted at every part.

What is **approximated**: framework shapes are matched by name, so an unrelated type with a
matching simple name is matched too, and a genuinely renamed one is not; `paramType` is the
minimal display form a developer would write (`ILogger<OrderService>`, `IFoo?`) while
`paramTypeFqn`, `fqn`, `ifaceFqn` and `implFqn` are fully qualified; a route template that is not
a compile-time string constant is treated as empty; a `MapGroup` prefix is only followed through
a local, field or property whose declaration yields an invocation — an initialiser or, for a
property, an expression-bodied getter; and a group declared in **another project** is read as
syntax, since no model here can bind that tree, so its chain is followed through string literals
only — a non-literal `MapGroup` argument counts as unresolved rather than yielding a guessed
template, while a chain carrying no `MapGroup` at all has no prefix to lose and contributes one
silently.

What is **omitted**: the schema table is embedded whole, but this mode emits none of
`branch_point`, `param_source`, `method_call`, `exception_map`, `http_out`, `worker_processor`,
`queue_name`, `redis_publish`, `signalr_push` or `exchange_name` yet.

### Determinism and strictness

Facts are sorted by `file` (ordinal), `line`, `type` (ordinal), then the canonical single-line
JSON of the whole fact, and exact duplicates are dropped — which is what collapses the two
copies a source file linked into two projects would otherwise contribute. The document is
indented with two spaces, UTF-8 without a BOM, LF newlines, and ends with a newline. Two runs
over the same tree are byte-identical.

A recognised site whose type cannot be resolved — an error type, an anonymous type, a bare
`object` or `dynamic` message, a publish with neither an argument nor a type argument — yields no
fact and is counted instead. The run reports `facts: N facts, M unresolved -> <path>` on stderr,
and `--strict` turns a non-zero `M` into exit 2.

A document whose fact walk throws does not end the run: the file and the exception are reported
as `warning: facts: <file>: <type>: <message>` on stderr and the document counts as one
unresolved site, so the rest of the solution is still emitted and `--strict` still fails.

### Fixture

`fixtures/csharp-flowtrace` is a package-free solution whose framework types are local stand-ins,
so it restores and builds offline. It is the tree the command at the top of this section walks;
its output is committed as `fixtures/csharp-flowtrace/facts.json`, which CI regenerates with
`--no-git --strict` and diffs byte-for-byte, and which `tests/flowtrace_facts.rs` checks for
shape without a .NET toolchain. Regenerate the snapshot after any change to the walker or the
fixture:

```sh
dotnet run --project tools/scout-semantic --no-build -c Release -- \
    fixtures/csharp-flowtrace/Fixture.sln --root fixtures/csharp-flowtrace \
    --emit flowtrace-facts --facts fixtures/csharp-flowtrace/facts.json \
    --publish-calls SubmitJob --no-git --strict
```

## Compiler-facts protocol

`--emit compiler-facts` writes one JSON document consumed by devscout's own one-shot admission
path (`devscout compiler-facts run|import`) rather than by a person or a provider tool: fixed
identity/negotiation header fields, then compiler-verified symbol facts and diagnostics gathered
directly from Roslyn symbols and `Compilation.GetDiagnostics()` -- not from the syntax-plus-model
walk `refs.jsonl`/the flow-tracer facts document use, since this mode reports what the compiler
itself resolved rather than a resolver-facing fact.

The header carries: `format`/`contractVersion`/`artifactSchemaVersion` (the wire shape this build
writes -- `artifactSchemaVersion` is `2` since this document gained the `occurrences` key below);
`producer.engineRevision` (this build's own protocol revision, `"2"` since a `"2"` engine can emit
occurrence facts); `profile` (target/configuration/platform, from `--tfm`/`-p
Configuration=`/`-p Platform=`); `dependencyFingerprint` (the sha256 of this project's own
`packages.lock.json`, a literal constant recomputed by hand when that lock file changes); `context`
(the real per-compilation build-context envelope under `context.envelope.compilations`, the same
shape `--emit context`'s own top-level array carries, with `context.contextFingerprint` a summary
devscout's admission path recomputes from that envelope's own `identity`/`fingerprint` pairs rather
than comparing two producer-written copies of one value -- it reads nothing else from the envelope
body); `sourceSnapshot.headSha` (omitted under `--no-git`, the same convention `--emit
flowtrace-facts` follows); `capabilities.requested`/`.provided` (`occurrences` is on by default, in
the supported set unless narrowed by `--capabilities`); and `completion.terminal: true`, written
only once this run has actually finished -- the explicit marker devscout's admission path uses to
tell a truncated or killed run apart from a genuinely complete one.

`units.processed`/`units.missing` name every loaded project by `Name|tfm`; a project that failed to
load is `missing`, never `processed`. `coverage.state` is `incomplete`, with one
`{unit, reason}` entry per affected unit under `coverage.incompleteUnits`, whenever a processed
unit's compilation carries at least one error diagnostic -- the run still succeeds and writes the
document; nothing here is gated by `--strict`. A unit the offline restore (see Prerequisites) could
not complete reports its restore failure as this entry's `reason` instead of the first compiler
error -- the same `unrestored` root cause the build-context envelope's state table names, reported
here even without `--emit context`. Every diagnostic (error or warning) is reported
under `diagnostics`, each carrying its own file and line. `symbols` carries one entry per named
type (`member` absent) and one per its ordinary methods and constructors (`member` present),
each with the declaring assembly, fully-qualified type name, generic arity, a parameter-derived
overload signature, and its declaration file/line under the `utf16-code-unit` span-encoding
convention Roslyn's own line/character positions already use. `symbols` and `diagnostics` are both
sorted (file, then line, then a shape-specific tiebreaker) for reproducibility; two runs over the
same pinned inputs are byte-identical.

When the `occurrences` capability is provided, `occurrences.sites` carries one record per
invocation, member-access, conditional-member and bare-identifier reference site in the loaded
documents -- driven by `SemanticModel.GetSymbolInfo`, independent of `symbols`' own declared-symbol
walk (its own record type, its own accept filter, every method kind including constructors and
local functions). Each site carries the caller's and, when one binds, the bound target's complete
identity in the same shape `symbols` uses, including the same `type` encoding (Roslyn's
`SymbolDisplayFormat.FullyQualifiedFormat`, `global::` stripped, so a nested or generic type reads
identically in `symbols[].type` and in `occurrences.sites[].caller.type`/`.target.type`/
`.candidates[].type`) -- stated as its own `occurrences.identityEncoding` literal
(`"fully-qualified-display-format"`), the same way `spanEncoding` states the span convention; the
occurrence's own span (the full reference node) and
the bound name token's own start position, both under the stated `spanEncoding` -- 1-based lines,
0-based characters, UTF-16 code units, end exclusive; a resolution state (`confirmed`, `ambiguous`,
`unresolved`, `inaccessible` or `dynamic`) with the compiler's own raw `candidateReason` and, where
the compiler offers them, the full candidate set; the identity and fingerprint of the compilation it
was bound in (the exact same value as that compilation's own `context.envelope.compilations[i]`
entry); and the per-document content identity (`sha1:`, not a git blob hash) of the occurrence's own
document and of every document its bound target is declared in. Every site failing to bind is
recorded, never dropped, with an empty candidate set and a null target standing for an explicit
negative fact rather than an absent one.

### Fixture

`fixtures/csharp-compiler-facts` is a package-free fixture, deliberately shaped to exercise two
same-line occurrences, two overloads of one name, a failed binding, an unresolved site, and (in
`Callers.cs`/`Other.cs`) a cross-document call, a nested/generic caller, an inaccessible member and
a dynamic receiver (see its own README). Its output is committed as
`fixtures/csharp-compiler-facts/compiler-facts.json`, which CI regenerates with `--no-git` and
diffs byte-for-byte, and which `tests/compiler_facts_shape.rs` checks for shape without a .NET
toolchain. Regenerate the snapshot after any change to `CompilerFacts.cs`, `CompilerOccurrences.cs`
or the fixture:

```sh
dotnet run --project tools/scout-semantic --no-build -c Release -- \
    fixtures/csharp-compiler-facts/Fixture.csproj --root fixtures/csharp-compiler-facts \
    --emit compiler-facts --compiler-facts fixtures/csharp-compiler-facts/compiler-facts.json \
    --tfm net9.0 -p Configuration=Debug -p Platform=AnyCPU --no-git
```

## Multi-targeting

Roslyn splits a multi-targeting project into one `Project` per framework, named `Name(tfm)`.
Projects are grouped by project-file path; the reported `name` has the `(tfm)` suffix stripped
and the framework moves to the `tfm` field.

`--tfm` is repeatable, and every requested target must be a target the project actually declares
-- a single-variant project's own declared framework is checked exactly the same way a
multi-targeting one's variants are, so requesting an undeclared target against either kind of
project is refused rather than silently kept. `--emit context` reports the refusal as its own
`unsupported` record naming both the requested and the declared targets, with zero facts written
under that identity; `--emit oracle`/`flowtrace-facts` simply exclude that project from the run
(as if it had not matched `--projects`). With no `--tfm` at all, the kept variant is the
ordinal-least declared target name (`StringComparer.Ordinal`), deterministic across runs and
machines regardless of the order Roslyn happened to enumerate the variants in.

## Build-context envelope

`--emit context` writes **one** JSON document -- one record per requested, selected, or excluded
compilation identity -- to `--context` (`-` for stdout). Alongside the oracle/fact-document output
(`--emit oracle,context`) or on its own, from the same load.

Each record names the exact project, requested/effective target, configuration and platform;
references, imported build files and their content hashes; language options and preprocessor
symbols; generated and linked documents; SDK/MSBuild/compiler/engine versions; raw workspace and
compiler diagnostics; the expected-versus-loaded document inventory with every difference
classified (`missing`, `linked-outside-root`, `skipped-directory`, `out-of-scope`) plus a
`documents.inventoryAvailable` marker (below); a context fingerprint; and one of five states,
always paired with a machine-readable reason:

| State | Reason (examples) | Meaning |
|---|---|---|
| `complete` | `complete` | Every expected document loaded, no compiler error, no unresolved reference, and the independent inventory was available. |
| `partial` | `unrestored`, `binding-error`, `missing`, `linked-outside-root`, `skipped-directory`, `out-of-scope`, `workspace-failure`, `inventory-unavailable` | A non-null compilation whose inventory or diagnostics are incomplete. `unrestored`: the offline restore (see Prerequisites) failed for this project in this run, or its existing assets file records an earlier restore error -- named ahead of `binding-error` even though the same missing `PackageReference` types also produce compiler diagnostics, since the restore failure is the record's root cause. |
| `unsupported` | `undeclared-target` | A requested `--tfm` the project does not declare; zero facts under this identity. |
| `failed` | `project-not-loaded`, `workspace-failure` | A project the solution names but that never reached the workspace at all, or one Roslyn accepted but produced no compilation for. |
| `excluded` | `not-requested` | A declared variant that was not the deterministic selection when no target was requested, or a project a `--projects` filter left out. |

A non-null compilation is never `complete` by itself: any compiler error, any expected document
that did not load (for any of the four dropped-document reasons above, including a deliberate
exclusion like `out-of-scope`), any unresolved reference, or an independent inventory that could
not be obtained at all (`inventory-unavailable`, below) demotes the record to `partial`. `--strict`
fails (exit 2) unless every non-`excluded` record's state is `complete`.

The expected document inventory comes from a second, independent MSBuild evaluation -- parsing
the solution file directly and re-evaluating each project through its own fresh
`ProjectCollection`, never the ambient one the Roslyn workspace uses -- so a document (or a whole
project) the workspace silently drops is still reportable, and a document Roslyn's own walk
tolerantly "loads" with empty content (a `Compile` item whose file was never created) is still
named missing. See `tools/scout-semantic/ContextInventory.cs`. When that independent evaluation
itself throws (for example, when MSBuild cannot evaluate a project in process),
`documents.inventoryAvailable` reads
`false` and, unless a stronger reason (a compiler error, an unresolved reference, a dropped
document) already demotes the record, its state is `partial`/`inventory-unavailable`.
`inventoryAvailable` is written explicitly on every record, `true` or `false`, and stays `false`
even when a stronger reason wins the record's own `state`/`reason` -- so a consumer reading only
`documents.expected`/`documents.dropped` can still tell "nothing was missing" from "we could not
check", which the single `reason` field alone cannot say once something else has already claimed it.
Completion is earned, never inferred from "we could not check".

A `--projects <glob>` filter that leaves a project out never reports it `failed`: it is `excluded`/
`not-requested`, the same state and reason an unselected multi-target variant gets, because both
are the caller's own deliberate exclusion rather than a load failure. This applies uniformly to a
project the filter skipped before target selection and to a solution-declared project that never
reached the workspace at all (`Vanished`-shaped): either way, a name the filter would also have
excluded is `excluded`/`not-requested`, not `failed`/`project-not-loaded` -- only a name the filter
admits, and that still never loaded, is genuinely `failed`. `excluded` records never affect
`--strict`'s rollup, so a `--projects`-scoped run that is otherwise healthy still exits 0. A
`--projects` value that matches no project at all is different: nothing loads and nothing is even
`excluded` under an identity, which is the same "zero projects loaded" usage error (exit 3) every
other `--emit` mode already reports for that input, and no envelope is written.

`diagnostics.compiler` carries only `Severity == Error` diagnostics: it exists to drive the
`binding-error` state (any compiler error demotes the record), not as a general warnings feed. A
generator's own diagnostics, of any severity, are inventoried separately, under
`generated.diagnostics` (below).

The fingerprint is one SHA-1 over reference identity, import content hashes, analyzer reference
identities, generator input (`AdditionalFiles`) content hashes, preprocessor symbols,
binding-relevant language/compiler options, SDK/MSBuild/compiler versions, the project's own
narrow build identity (configuration, platform, target, assembly name, root namespace -- never the
whole project file), and every project reference's own already-computed fingerprint. Two distinct
targets or configurations of one project are always distinct identities with distinct
fingerprints. A restore-generated import under any project's own `obj/` directory (such as
`*.nuget.g.props`/`.targets`) is excluded from both `imports` and the fingerprint: those files
embed the local machine's absolute NuGet global-packages root, so folding them raw would make the
fingerprint depend on where the repository happens to be checked out; a package version change is
already visible through the resolved metadata references themselves. Computing the fingerprint
performs a second, independent evaluation of the project (and, transitively, its own project
references); a run logs `context: fingerprint computation totaled <N>ms across <M> compilations`
to stderr so that cost is observable rather than assumed. See
`tools/scout-semantic/ContextFingerprint.cs` and `fixtures/csharp-context-fingerprint/README.md`
for a committed before/after envelope pair per mutation class.

Source-generated documents are inventoried via the Workspace API's own
`Project.GetSourceGeneratedDocumentsAsync()`, which names each document's `HintName` but exposes
no generator-identity property at all (verified against the restored 4.14.0 assemblies), so
`generated.documents[].generator` is always `"unknown"`. `generated.diagnostics` carries any
diagnostic (any severity) located on one of those same generated documents' own syntax trees,
distinguished from `diagnostics.compiler` by tree identity rather than by guessing from a file
path; it is `[]` whenever no generator in the run reported one, which is the common case.

`schemaVersion` starts at `1` and is a separate counter from the flow-tracer fact document's own
`schemaVersion` -- two different documents, two different emit modes, two different output paths.

External imports (an SDK `.props`/`.targets` file, a NuGet package's build file) are never
recorded by absolute local path: `imports[].identity` is a normalized identity (package id and
version when the well-known NuGet global-packages path shape is recognizable, else the file's bare
name) plus a content hash, and `ContextSchema.Validate` rejects an absolute path in any
path-shaped field as defense in depth. The same guard also scans every free-text diagnostic
message (`diagnostics.workspace[].message`, `diagnostics.compiler[].message`,
`generated.diagnostics[]`) token by token, because a build tool's own diagnostic text can quote an
absolute path where a path-shaped field never could -- the writer already root-relativizes (or, for
a path outside the analysed repository, reduces to a bare file name) any absolute path it finds
in a workspace diagnostic's message before this guard ever runs, so the guard is defense in depth,
not the only line of protection.

Fixture: `fixtures/csharp-context/`, with its own `README.md` and five committed envelope
snapshots CI regenerates and diffs byte-for-byte, covering all five states and all four document
drop reasons; `tests/context_envelope.rs` pins the shape offline, without a .NET toolchain. The
fingerprint's own mutation-class evidence and the four-way multi-target/multi-configuration
identity round trip live in the sibling `fixtures/csharp-context-fingerprint/` (its own `README.md`;
`tests/context_fingerprint_pairs.rs` pins it offline the same way), generated locally and not
CI-regenerated.

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
- Source-generated documents are not walked by `oracle`/`flowtrace-facts`; only files on disk are.
  `--emit context` inventories them separately (see [Build-context envelope](#build-context-envelope)).
- The offline restore is skipped, with a `warning: restore:` line on stderr, for a project whose
  assets file location neither the in-process MSBuild nor the SDK's `dotnet msbuild` can evaluate,
  or when the `dotnet` host cannot be found beside the registered MSBuild instance: guessing a
  location could re-restore a project whose real assets file lives elsewhere. Such a project keeps
  whatever state its compilation produces. Any failure of the in-process evaluation, not only a
  project error, falls back to the SDK's `dotnet msbuild`; only running out of memory ends the run.
  Evaluating a project out of process costs one `dotnet msbuild` start (about a second) per
  project the in-process MSBuild cannot evaluate.

## Packages

Pinned in `packages.lock.json` and restored with `--locked-mode`:

| Package | Version |
|---|---|
| `Microsoft.Build.Locator` | 1.9.1 |
| `Microsoft.CodeAnalysis.CSharp.Workspaces` | 4.14.0 |
| `Microsoft.CodeAnalysis.Workspaces.MSBuild` | 4.14.0 |
| `Microsoft.Build` | 17.7.2 (`ExcludeAssets="runtime"`) |
| `Microsoft.Build.Framework` | 17.7.2 (`ExcludeAssets="runtime"`) |
| `Microsoft.Build.Tasks.Core` | 17.7.2 (`ExcludeAssets="runtime"`) |
| `Microsoft.Build.Utilities.Core` | 17.7.2 (`ExcludeAssets="runtime"`) |
| `Microsoft.NET.StringTools` | 17.7.2 (`ExcludeAssets="runtime"`) |

4.14.0 is the newest 4.14.x release and the Roslyn line that ships with the 9.0.3xx SDK.
Bumping to a 5.x line requires a matching newer SDK and a lock-file refresh
(`dotnet restore tools/scout-semantic --force-evaluate`).

The registered SDK supplies every MSBuild-family assembly: `Microsoft.Build`, each
`Microsoft.Build.*` package except `Microsoft.Build.Locator`, and `Microsoft.NET.StringTools`. All
five are already transitive dependencies of `Microsoft.CodeAnalysis.Workspaces.MSBuild` (StringTools
also through `Microsoft.Build` itself); pinning them explicitly at the resolved version with
`ExcludeAssets="runtime"` adds no new package and no version bump, but keeps their DLLs out of the
build output and out of `scout-semantic.deps.json`. The in-process `ProjectCollection`s (the
restore step's assets-file evaluation and `ContextInventory`) then resolve every one of them
through `MSBuildLocator`'s redirect to the installed SDK at run time. The locator can redirect only
an assembly the tool does not ship itself: an app-local older copy binds first, so a newer
registered MSBuild fails on a member that copy lacks (a 10.0 SDK's evaluator on StringTools 17.7.2,
for one), and an older `Microsoft.Build` throws on an intrinsic function the installed SDK's
targets call. A test reads the tool's dependency manifest and names any MSBuild-family library that
still carries a runtime asset.
