# Limitations

Known, rather than hidden. One line per limit; each links the page that details it.

- `refs` and `impact` fold none of the TypeScript edge kinds, so they list no TypeScript inbound rows: [TypeScript imports](explanation/resolution.md#typescript-imports).
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
- A message-bus row is a possible route from a recognised publish or dispatch call to a matching handler; runtime routing is unverified: [`bus-hop`](reference/json-output.md#bus-hop).
