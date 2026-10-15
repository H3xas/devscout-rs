# Limitations

Known, rather than hidden. One line per limit; each links the page that details it.

- TypeScript references cover imports, calls, JSX tags and dispatches only; declared module modes resolve repository workspace packages, but external dependencies and unrecorded `require()` forms remain outside the graph, configuration-only edits need another `map`, and JavaScript query answers have no separate parity measurement: [TypeScript modules](reference/typescript-modules.md).
- Rust edges come from path resolution only: a method call is never typed by its receiver, and trait dispatch, generics, macro expansion and `cfg` evaluation are not followed: [Rust known gaps](reference/rust-constructs.md#known-gaps).
- In a mixed repository a Rust item shares the symbol lookup with C# and TypeScript definitions, so a shared bare name can become ambiguous: [Rust paths](explanation/resolution.md#rust-paths).
- Generic-delegate `typeParams` handling is under review rather than pinned: [Generic delegates](explanation/resolution.md#generic-delegates).
- Heuristic `uses-member` edges carry a `tier`, `ext` or `guess`, and neither tier's edges become the premise of a further `impact` hop: [Heuristic tiers](explanation/resolution.md#heuristic-tiers).
- Receiver typing leaves some shapes unrecorded (a bare unqualified call, a deep chain, a `dynamic` receiver, a `using static` import): [Receiver typing](explanation/resolution.md#receiver-typing).
- Some lambda parameters stay untyped (a callee outside the graph, a generic delegate parameter, a named argument): [Lambda parameters](explanation/resolution.md#lambda-parameters).
- Three shapes bind the wrong side of an `inherits` edge and are pinned as known false positives: [Declaring-type binding](explanation/resolution.md#declaring-type-binding).
- A receiver typed from a `foreach` variable or a lambda parameter reads as having no type arguments: [Type-argument counts](explanation/resolution.md#type-argument-counts).
- A fully qualified name never falls back to its bare last segment: [Qualified names](explanation/resolution.md#qualified-names).
- A static qualifier walks nested types one segment at a time, and `nameof(Outer.Inner)` emits no `uses-member` edge: [Nested-type qualifiers](explanation/resolution.md#nested-type-qualifiers).
- Only the two-type-argument DI registration shape records an `implements` edge, and an ambiguous arity match emits nothing: [DI registrations](explanation/resolution.md#di-registrations).
- The project model reads only `.csproj` and `Directory.Build.props`, with no MSBuild evaluation, no NuGet resolution and no `.sln`: [Project model](explanation/resolution.md#project-model).
- Conditional compilation uses the no-build symbol model: no symbol is predefined and `DefineConstants` is not read: [Conditional compilation](explanation/resolution.md#conditional-compilation).
- Compiler-fact enrichment changes what the `precise` tier holds, so its own precision figure is not comparable with a syntax-only run: [Compiler facts and the precise tier](explanation/resolution.md#compiler-facts-and-the-precise-tier).
- Which C# constructs produce facts is catalogued, not implied, and the silent rows are listed as follow-ups: [C# constructs](reference/csharp-constructs.md#follow-ups).
- Which .NET targets, project systems and framework profiles are qualified is catalogued separately, for the compiler-fact path only: [.NET targets](reference/dotnet-targets.md).
- The content store follows `HOME` by default: [Environment variables](reference/environment.md#the-content-store-follows-home).
- No watch mode: `map` is incremental, but you run it: [Freshness](concepts.md#freshness).
- Artifact ordering is deterministic but not a stability promise across minor versions: [Storage](reference/storage.md#ordering).
- Freshness validation is bounded by three caps — 5,000 `git status` entries, 2,048 candidate paths and 16 MiB of candidate bytes — and answers `unknown` with reason `validation-budget` past any of them: [Freshness](reference/json-output.md#freshness).
- The index belongs to one worktree at a time: a query from any other worktree reports `unknown` with reason `other-worktree` until it runs `map`: [Storage](reference/storage.md#one-owner-per-index).
- Freshness is undefended for files Git ignores, which the walk can still index, so their edits and their new ones go undetected: [Freshness](concepts.md#freshness).
- Freshness is undefended for paths `git status` cannot see — `assume-unchanged`, `skip-worktree`, sparse checkout, submodules, or a stale `core.fsmonitor` answer: [Freshness](concepts.md#freshness).
- Freshness is undefended outside a Git repository, in a bare repository, and under `GIT_DIR`/`GIT_WORK_TREE` overrides: [Freshness](concepts.md#freshness).
- Freshness is undefended for an edit made after validation starts while a query runs, for manifest entries an earlier broader `map` left outside the published scope, and for deliberately colliding digests: [Freshness](concepts.md#freshness).
- Artifact identity is `len`/`modified_ns` (plus `dev`/`ino` on Unix), so a foreign write of the same length inside one timestamp tick on Windows, or inode reuse with an equal size and nanosecond mtime on Unix, goes unnoticed: [Freshness](concepts.md#freshness).
- An index published at the same tree by a different build of this or a later release is not told apart by extractor identity; compiler-fact artifacts and imported cross-repository edges keep their own freshness rules: [Freshness](concepts.md#freshness).
- Latency on slow or network filesystems, and dirty trees beyond the benchmarked 1,000-file reference state, are outside the measured budget: [Freshness](concepts.md#freshness).
- Freshness validation reads `git status` without allowing Git to write a refreshed index, so a tree whose index went stale without the user running Git pays a full re-hash on every query: about 0.5 s on the benchmarked corpus against about 0.1 s when the index may be written back: [Freshness validation latency](benchmarks/results/2026-10-14-freshness-latency.md#what-this-bounds).
- A message-bus row is a possible route from a recognised publish or dispatch call to a matching handler; runtime routing is unverified: [`bus-hop`](reference/json-output.md#bus-hop).
