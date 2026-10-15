# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Test files named `*.test.*` or `*.spec.*`, or placed below `__tests__`, are now classified by
  path for `.mts`, `.cts` and `.jsx` as well as `.ts`, `.tsx`, `.js`, `.mjs` and `.cjs`. The
  `tests` and `impact` commands count these files when they reference the queried code.

### Fixed

- **The two committed .NET oracle snapshots are now reproducible from one declared SDK install,
  and the tests that build temporary projects select it explicitly.** `compiler-facts.json` and
  `context.json` record the resolved SDK and MSBuild versions, every reference assembly's MVID,
  the hash of every imported file and the fingerprint folded from all of it, so two installs of one
  SDK patch -- a macOS installer layout and a `dotnet-install` tarball layout, or a machine that
  resolves a newer default -- disagreed on their bytes with no source change. `tools/dotnet-snapshots.py`
  is now the only producer of those two files: it validates the supplied `--sdk-root` against the
  new pin in `tools/dotnet-snapshot-sdk.json` (exact SDK, the reference pack's and the workload
  manifests' own content inventories, the published archive digest per host) and refuses anything
  else before either file changes, then regenerates both and compares raw bytes with nothing
  masked. CI extracts the declared archive into a root of its own and runs the same check instead
  of trusting the SDK the runner selects, and every other step of that job which re-produces one of
  the two snapshots -- the compiler-facts and context producers, the capability-restricted run and
  the repeat-run check -- invokes the oracle through that root too, so no producer is left on the
  runner's own install of the same version. `OfflineRestoreCliTests` and the tests sharing its
  helpers take their host, their `global.json` pin and their stub's reference assemblies from
  `SNAPSHOT_DOTNET_ROOT`, so a project they create under the system temporary directory compiles
  under the pinned install whether or not the machine's own default SDK is newer. Both the tool and
  those helpers drop the
  SDK-selection variables a host exports to its children (`MSBuildSDKsPath`, `MSBuildExtensionsPath`,
  `DOTNET_HOST_PATH` and the `DOTNET_MSBUILD_SDK_RESOLVER_*` set) before applying the pinned root:
  a host keeps a value it finds already set, so a 10.0-band launcher would otherwise hand its own
  targets and tasks to the pinned child and fail the offline restores it was meant to pin.

- A query no longer reports `fresh` for an index that does not describe the working tree. Comparing
  `HEAD` and dirty-path membership was not enough: a file that was already dirty when the index was
  built was never re-checked, so editing it a second time -- even keeping its size and modification
  time -- left the old answer labelled `fresh`.

- **A qualified name written with spaces, line breaks or comments between its parts now resolves like
  its plain spelling.** Every dotted name the C# extractor records -- a receiver's declared type, a
  type reference, a base-list entry, a DI registration's two types, a namespace name, and a `using`
  target or alias -- is rebuilt from the name's own tokens, so `Widgets . Widget`,
  `Widgets./*c*/Widget` and `Widgets.` followed by a line break and `Widget` all read as
  `Widgets.Widget`. The raw source text used to reach the resolution ladder, match no def path, and
  drop a precise edge to a guess. A name whose subtree holds a parse error keeps its source text and
  so binds nothing new. This first moved the fragment cache to the `fragments-v34.json` /
  `fragments-index-v34.json` generation; the handler change below advances the release's current
  cache to v35, so an existing cache re-extracts once.

- **An interpolated string argument binds an in-graph `[InterpolatedStringHandler]` parameter.** A call
  whose argument is written `$"…"` — regular, verbatim (`$@"…"`, `@$"…"`) or raw (`$"""…"""`), with or
  without holes, parenthesized or not — is recorded apart from a plain string and is admitted at a
  parameter whose type resolves to a type declared with `[InterpolatedStringHandler]` (bare, with the
  `Attribute` suffix, namespace-qualified or `global::`-qualified, on any one of a type's partial
  declarations), so the call binds the handler parameter's declaring type precisely again. Every other
  comparison still reads the argument as `string`: a string literal or a string-typed local against a
  handler parameter that declares no implicit conversion keeps the argument-type veto, an interpolated
  string against an unmarked in-graph parameter keeps it too, so the calls that bind an extension method
  behind an instance overload taking a `StopContext` bind it as before. The fragment cache moves to
  `fragments-v35.json`, so an existing cache re-extracts once.

### Changed

- An index belongs to one worktree at a time. A `map` records the worktree that published it, and a
  query from any other linked worktree answers `unknown` with reason `other-worktree` instead of
  serving that index as `fresh`. Fragment reuse no longer crosses worktrees either, and the cache
  generation moves on to `v36`, past both generations above.
- Freshness now compares content. The index records a SHA256 of the bytes it extracted for every
  indexed file and every configuration input it read, and validation hashes each candidate and
  compares. Size and modification time are budget estimates only. `changedFiles` counts files whose
  bytes differ, plus added and deleted relevant files and changed configuration inputs.
- The `freshness` object gains three reason words -- `other-worktree`, `validation-budget` and
  `index-changed` -- and `git-unavailable` widens to cover a `git status` run that fails or cannot be
  parsed. No new state and no new key. Each new reason also gets its own stderr line; the stale line
  keeps its earlier wording, and stderr stays silent for a missing record and for unavailable Git.
- Stderr and `--json` come from one validation per invocation, so a query pays for one `git status`
  and cannot describe two different moments.
- Validation is bounded: 5,000 `git status` entries, 2,048 candidate paths and 16 MiB of candidate
  bytes. Past a cap the state is `unknown` with reason `validation-budget`.
- A `map` holds a lock in the shared scout directory and publishes the freshness record last, by
  atomic rename, after an in-progress marker. A query racing a map, or reading a record whose
  artifacts were replaced afterwards, answers `unknown` with reason `index-changed`.
- `SCOUT_MTIME_REUSE` is accepted and ignored: reuse keys are content-derived unconditionally, so
  there is no mtime mode left to select. The `MapOptions::hash_reuse` field stays for API
  compatibility and no longer changes behavior.
- The `manifest` module's re-exported `freshness_warning`, `read_index_state`, `write_index_state`,
  `index_state_path`, `dirty_indexed_files_at` and `is_working_tree_dirty` move to the
  `freshness` module's own submodules.
- An index written by an earlier release, or by a different build of this one at the same tree, is
  never certified. Queries answer `unknown` with reason `no-index-state` and a note to re-run `map`,
  and the first `map` after an upgrade is cold. A downgrade needs a re-map too. See the
  [upgrading guide](docs/guides/upgrading.md).

## [0.9.0] - 2026-10-12

A query-answer and second-language release: every resolved answer states its full count and the next
verb to run, `refs`, `read`, `impact` and `tests` answer for TypeScript and for Rust, `.mjs`, `.cjs`,
`.mts` and `.cts` modules are indexed, the opt-in enriched lane reports when it degrades, and the
precise tier fixes each wrong-target mechanism measured in 0.8.0, removing 115 of its 170
wrong-target edges on the pinned benchmark corpus.
The release tree is measured in [`docs/benchmarks/results/2026-10-12-release-0.9.0.md`](docs/benchmarks/results/2026-10-12-release-0.9.0.md).

### Added

- **Rust definitions are indexed for `find` and `read`.** `map` now indexes every `.rs` file in
  its scope, in every repository, and records each item: modules, structs, enums, unions, traits,
  functions and methods (free, associated, trait and extern), constants, statics, type aliases and
  associated types, and `macro_rules!` macros. `find` lists them, and `read` quotes an item's whole
  span, doc comments and attributes included. An item is named by its name, by its id
  (`<file>#<path>`, such as `src/shelf.rs#Widget::new`), or by a `::` path such as `Widget::new`;
  when two items in a file share a path, the later ones take a `-1`, `-2` suffix. This is the
  definition layer used by the Rust references and tests described below. That first slice
  used the `fragments-v28.json` cache generation, superseded by v33 in this release.
- **`refs`, `impact` and `tests` answer on Rust code.** `map` now resolves Rust paths through the
  module tree (`mod` declarations, `mod.rs` and `foo.rs` with `foo/` layouts, inline modules, and
  crate roots read from each `Cargo.toml` in scope, or `lib.rs` and `main.rs` without one) and
  through `use` scope (groups, globs, renames, `self`, `super`, `crate`, the package's library name,
  and `pub use` re-exports followed to the defining item). A call bound to one function becomes a
  `uses-member` edge to it, a type use, struct literal or constructor a `uses-type` edge, and
  `impl Trait for T` an `implements` edge; calls written inside macro arguments such as
  `assert_eq!` and `vec!` count too. A name two imports both provide gives an `ambiguous` edge
  instead of a guess, and a method call is never typed by its receiver. Functions carrying
  `#[test]` are recognized as tests, so `tests <fn>` and `impact` reach them. `init` reports `.rs`
  files as "indexed and graphed, path-resolved edges". A Cargo package model is kept beside the
  graph in `rust-crates.json`, so editing a `Cargo.toml` rebuilds the graph. Measured against
  rust-analyzer's own references on this repository, call edges scored precision 1.0 and recall
  0.999. This slice used the `fragments-v29.json` cache generation, superseded by v33 in this
  release.
- **Rust edges reach cargo's nested targets, `cfg_attr` tests and `#[path]` modules.** Cargo's
  nested target files `tests/<name>/main.rs`, `examples/<name>/main.rs` and
  `benches/<name>/main.rs` are crate roots, so their calls through the library's crate name bind
  like those from `tests/*.rs`. A function marked by `#[cfg_attr(<predicate>, test)]`, with any
  predicate and nested `cfg_attr` included, is a test for `tests` and `impact`. A `mod x;`
  carrying `#[path = "..."]` reaches the file the attribute names, as rustc loads it, and never the
  default `x.rs` beside it that rustc does not compile; a `path` carried by `cfg_attr` reaches
  nothing. Measured against rust-analyzer on a fixture package holding all three shapes, every
  call through them is recalled with no contradicted edge, and this repository's own scores are
  unchanged. This slice used the `fragments-v30.json` cache generation, superseded by v33 in this
  release.

- **Every resolved query answer states its full count and the next verb to run.** `refs`, `read`,
  `impact` and `tests` now open their text and `--compact` output with the number the answer reached
  before its row cap, `N inbound (showing M)` or `N files (showing M)`, so a truncated table can no
  longer read as a complete one; a zero-hit answer says `0 inbound (showing 0)`. The same four
  verbs, and `find` in text, end with a `next:` line naming the cheapest follow-up verb,
  single-quoted where a shell would otherwise read an argument as its own. Under `--json` the same
  four verbs carry the suggestions as a `next` array of argv arrays, immediately before `outcome`.
  Each suggestion is resolved through the resolver the named verb itself uses, so every command the
  line offers runs. `schema_version` stays 1: this adds a key and a line rather than changing one.

- **`refs`, `read`, `impact` and `tests` answer for TypeScript.** The `import`, `call`, `jsx-use`
  and `dispatch` edges the graph already held are now read: `refs` and `read` list each kind in a
  table of its own, under the shared inbound cap, and `refs --out` mirrors them outbound; a table
  appears only when it has rows, so C# answers keep their bytes. Rows carry the new `why` words
  `call`, `jsx-use`, `dispatch` and `import`. `impact` walks the four kinds at every hop, and they
  count toward the hub brake. A TS `import` edge in `graph.json` gains `bound_defs`, the
  definitions its bindings resolve to, so an import row names the exact symbol. `tests` counts a
  `.ts`/`.tsx` file named `*.spec.ts(x)`, `*.test.ts(x)` or placed below `__tests__/` as a test file
  (`via: "path"`, `why: "test-path"`). JavaScript files answer through the same edges but are not
  measured. This slice used the `fragments-v31.json` cache generation, superseded by v33 in this
  release.

- **`.mjs`, `.cjs`, `.mts` and `.cts` modules are indexed.** `map` discovers the four module
  suffixes: `.mjs`/`.cjs` as JavaScript, `.mts`/`.cts` as TypeScript. A repository built from them
  yields definitions for `find`, and its relative imports bind through the same `import` and `call`
  edges as `.ts`/`.js`. An extensionless or directory specifier resolves to any of the four,
  keeping a TypeScript source ahead of the JavaScript it compiles to; a specifier naming an emitted
  `.mjs`/`.cjs` still falls back to its `.mts`/`.cts`/`.ts` source, and a literal file always wins.
  `init` reports the four suffixes in its indexed-and-graphed census, and an existing repository
  gains the files on its next ordinary `map`.

### Fixed

- **An inherited field or property wins over a same-named type in a bare member access.**
  `RoutingSlip.Itinerary` inside a derived courier context now binds through its inherited
  `RoutingSlip` property instead of an imported `RoutingSlip` type. Explicit type paths, generic
  qualifiers and local receivers keep their existing lookup rules.
- **Calls skip same-count overloads that reject known argument types.** A typed receiver now
  continues through its base interfaces or classes, and an applicable extension can bind when
  the receiver's own overload cannot accept a written literal, declared value or member result.
  Unknown types still leave the overload available. The fragment cache moves to
  `fragments-v33.json`, so an existing cache re-extracts once.
- **Compiler-facts and context output list compiler diagnostics in one fixed order.** The
  compiler returns its diagnostics in an order that can change between identical runs. Each
  context record's `diagnostics.compiler` array, also embedded in compiler-facts output, was
  written in that order, and the compiler-facts `diagnostics` array kept it for entries sharing a
  file, line and code, so two runs over the same inputs could write different bytes. Both
  arrays are now ordered by file, line, diagnostic code, message and severity, with a missing
  file or line first and every text compared ordinally, so repeated runs over the same inputs
  write identical bytes.
- **An explicit interface implementation is no longer treated as an ordinary member of the type
  that declares it.** C# reaches `void IFace.Run()` or `IHost IBus.Host` only through a receiver
  typed as the interface, so inside the implementing type a call, a property hop or a bare name now
  binds what C# binds: `this.Run()` binds a base class's public `Run` or earns no precise edge
  instead of pointing at the invisible explicit method, `bus.Host.Open()` reads the public `Host`
  property's type even when the explicit one is declared first, and a bare `LogContext.Info` names
  the `LogContext` type rather than an explicit property of that name. An explicit implementation is
  still found through the interface, by `find` and `read`, and by `implements` pairing. The
  fragment cache moves to `fragments-v24.json`, so an existing cache re-extracts once.
- **A property read through an interface that inherits two declarations of it binds the one C#
  binds.** When an interface reaches the same property along two base paths and one declaring
  interface derives from the other, the more derived declaration hides the other, with or without
  `new`, however deep either one sits. A read such as `cfg.Topology` on an
  `IInMemoryBusConfiguration : IBusConfiguration, IInMemoryEndpointConfiguration` receiver now
  binds `IInMemoryEndpointConfiguration.Topology` instead of the hidden
  `IEndpointConfiguration.Topology` that the first base reaches. Method calls and class receivers
  bind as before.
- **A bare type name resolves in the namespace-level order C# uses.** A type declared in an
  enclosing namespace now wins over a same-named type imported by a using directive declared
  further out, including compilation-unit and `global using` directives: `TestInstance` inside
  `namespace Tests { namespace ContainerTests { using TestFramework.Sagas; ... } }` binds the
  `TestInstance` declared in `ContainerTests`, not the imported one. A using directive declared
  inside a namespace still wins over a type in a shallower namespace, a using declared in an inner
  namespace wins over one declared in an outer namespace instead of making the name ambiguous, and
  a using declared in a namespace block that does not enclose the reference no longer applies to
  it. With a `.csproj` project model, a type in an enclosing namespace, or one a using imports, that
  is declared only in a project the reference's own project does not reference is passed over, so
  the lookup moves on as C# does. A publish that names its message with a qualifier
  (`Publish(new Subjects.StartCommand())`) now resolves the message through that qualifier instead
  of the bare name. The fragment cache moves to `fragments-v25.json`, so an existing cache
  re-extracts once.
- **`map` says when the enriched lane was not applied, and why.** A stale or unreadable admitted
  compiler-facts artifact used to drop `map` to the syntax lane with nothing on screen to say so.
  Whenever an entry sits at the compiler-facts path or `--no-semantic` is given, `map` now prints a
  second line, `lane: enriched` or `lane: syntax (enriched lane not applied: <reason>[: <detail>])`,
  and writes the same record to `graph.json` as `stats.lane` (`state`, then `reason` and `detail`
  when present). The reasons are `stale-artifact`, `imports-moved`, `artifact-unreadable` and
  `no-semantic`, and `detail` carries the freshness token verbatim. With no artifact and no flag,
  output and `graph.json` are unchanged.
- **A stale or moved-import artifact no longer writes a zeroed `stats.semantic` block.** `audit`
  read that block as the enriched lane; such a graph now reports `lane: syntax`, as the documented
  omit-when-not-applied rule already promised. Edges are unchanged.
- **`map --no-semantic` always rebuilds**, so its lane record describes the current run. With no
  artifact on an unchanged tree it now prints `graph rebuilt` where it printed `graph unchanged`.
- **A call written with an explicit type-argument list binds only an overload with that many type
  parameters.** `configurator.AddConsumer<TConsumer, TDefinition>()` used to bind a same-named
  `AddConsumer<T>` the receiver's interface declares, because only the value arguments were
  counted. C# refuses that overload, and now so does the precise member check: a written list
  binds only a method whose own overload declares that many type parameters and also accepts the
  value arguments. Extension methods are filtered the same way, so the call binds the extension
  class that declares `AddConsumer<T, TDefinition>` rather than one that declares only
  `AddConsumer<T>`. A call written without a list binds as before. The fragment cache moves to
  `fragments-v26.json`, so an existing cache re-extracts once.
- **A lambda argument binds only an overload it can convert to.** `list.Any(m => m switch { ... })`
  used to bind a same-named `Any(Action<Filter> apply)` or `Any<T>(FilterDelegate<IX<T>> filter)`
  overload, because only the lambda's parameter count was compared with the delegate's. C# refuses
  both, and now so does the precise member check: a lambda whose body can only produce a value
  skips an overload whose delegate returns `void`, and a generic overload is skipped when the call
  writes no type-argument list and one of its type parameters occurs only in the parameter types
  of delegates that implicitly typed lambdas fill, since nothing can infer it. Every lambda
  argument must fit the same overload. The call then binds the base type's overload that accepts
  the lambda, and a call through a property (`harness.Sent.SelectAsync(_ => true)`) no longer binds
  an overload the lambda cannot convert to. A statement-bodied lambda, an explicitly typed one, a
  call with a written type-argument list, and a type parameter another argument or the delegate's
  return can fix bind as before. The fragment cache moves to `fragments-v27.json`, so an existing
  cache re-extracts once.
- **A NuGet vulnerability-audit warning no longer makes a compilation partial.** A restore done
  offline with the audit on logs `NU1900` for each project, and the SDK replays it as a workspace
  failure, so every such compilation read `partial`/`workspace-failure`, `--strict` exited 2, and
  the enriched lane confirmed little or nothing. `scout-semantic` now keeps such a diagnostic in
  the record's workspace diagnostics without demoting the compilation or failing `--strict`,
  when the assets file of the project it names logs `NU1900` to `NU1905` at `Warning` level with
  the same message. Any other code or level, and a missing or unreadable assets file, still
  demote. Artifacts produced earlier keep their states until they are produced again.
- **Compiler-facts coverage and the lane no longer claim complete or enriched over partial
  compilations.** `compiler-facts import`, `run` and `status` printed `coverage: complete`
  whenever no unit was listed incomplete, and `map` and `audit` said `enriched`, even when the
  producer had marked compilations partial and their facts confirmed nothing. The coverage line
  now reads `coverage: partial (K of M compilations: <tally>)`, and the incomplete-units wording
  gains the same tally. See Changed for the new lane values.
- **`audit --json` errors go to stderr.** A fatal error (a bad argument, no `graph.json`, an
  unreadable `--assert` file) was printed on stdout, so a CI step that redirected stdout into the
  report file showed no message. With `--json` anywhere in the arguments, every error now goes to
  stderr with exit 1 and stdout stays empty. Text mode is unchanged.
- **A receiver declared with a qualified type binds that type, not a same-named one.** A pattern
  such as `endpoint is Outbox.OutboxSendEndpoint outboxEndpoint` or a field typed
  `ConfigurationHostSettings.ConfigurationBatchSettings` was recorded by its last segment alone, so
  `outboxEndpoint.Endpoint` resolved through a `using` that named a different `OutboxSendEndpoint`,
  and `_settings.Enabled` bound a top-level `ConfigurationBatchSettings` in a project the declaring
  code never referenced. The written path is now recorded alongside the simple name and resolved
  first, so the member binds the declared type; a path that matches no type, or two, binds neither.
  Unqualified receivers, extension matching, generic arity and the nullable rules are unchanged, and
  a same-named declaration without a written path behaves exactly as before. This slice used the
  `fragments-v32.json` cache generation, superseded by v33 in this release.

### Changed

- **Documentation is organised by type.** `docs/answer-contract.md`, `docs/csharp-coverage.md`,
  `docs/dotnet-target-coverage.md` and `docs/call-site-evidence-matrix.md` moved to
  `docs/reference/json-output.md`, `docs/reference/csharp-constructs.md`,
  `docs/reference/dotnet-targets.md` and `docs/reference/call-site-evidence.md`. Each old path
  keeps a one-line pointer for this release and is removed in the next minor release.
- **The lane record reads the admitted artifact's compilations.** `stats.lane` gains the state
  `partial` (reason `compilations-incomplete`, printed `lane: enriched, partial (<detail>)`, with
  `stats.semantic` kept) and the not-applied reasons `no-complete-compilation` and
  `nothing-confirmed` (no `stats.semantic`, and the same edges as the syntax-only graph). Every
  record decided from the compilations, `applied` included, carries
  `compilations: {"total": M, "partial": K}`. `audit` reports lane `enriched-partial` for a
  partial record, so a check for `"enriched"` now fails on such a run, adds the reason to its
  text header, and appends `lane_detail` and `compilations` to its `--json` report, which
  `--assert compilations.partial` reads. Exit codes are unchanged, and a graph without a lane
  record audits as before.

- **JavaScript test files are classified by their path.** `tests` and `impact` count a
  `.js`/`.mjs`/`.cjs` file named `*.test.*`/`*.spec.*`, or placed below an exact `__tests__`
  directory, as a test file (`via: "path"`, `why: "test-path"`), where only `.ts`/`.tsx` names were
  classified before. An attribute or test-project vouch still outranks the name, and `.jsx` and the
  `.mts`/`.cts` suffixes stay unclassified.

## [0.8.0] - 2026-09-25

A candidate-route release: a recognized publish or dispatch call gains `bus-hop` edges to the
matching handlers anywhere in the indexed repository, which `refs`, `read`, `impact` and `tests`
traverse and disclose as possible routes (`--no-bus` restores prior traversal), and `map` consumes
an admitted compiler-facts artifact at resolve time, while `map --no-semantic` or no admitted
artifact leaves the syntax-only graph exactly as before.

### Added

- **The compiler-facts producer restores a missing `PackageReference` assembly offline, before
  opening the workspace for facts.** A never-restored working copy used to leave every
  `PackageReference`-resolved type unresolved (`CS0234`/`CS0246`), even though `FrameworkReference`
  assemblies loaded fine, because `MSBuildWorkspace` evaluates but never restores. The engine now
  finds each project the workspace opened whose MSBuild-evaluated assets file is missing (asking
  the SDK's own `dotnet msbuild` when the in-process evaluator cannot load a project) and restores
  exactly those, one at a time, from the local NuGet global packages folder alone -- never a
  configured feed, a project's additional source, or the network, and never touching a project
  whose assets file already exists. `--no-restore` opts back out. A project whose restore failed,
  in this run or as recorded in its assets file by an earlier one, is `partial`/`unrestored`
  (ahead of the generic `binding-error`), and its compiler-facts `coverage.incompleteUnits` reason
  names the restore failure instead of the first compiler error.
- **Native message-bus candidate links across one repository.** A recognized publish or
  dispatch call whose message resolves to one in-graph definition gains a `bus-hop` to each
  matching handler declaration anywhere in the indexed repository. A project or
  `ProjectReference` is not a boundary: a publisher and its handler in different projects or
  assemblies are still connected. Both message identities resolve through the ordinary type
  ladder in their own file contexts, so a shared short name, an adjacent type mention or a
  method spelling alone supplies no message identity. A message and an array of that same
  message are distinct identities on both sides of a hop -- a single publish never reaches an
  array consumer and an array publish never reaches a single-message consumer -- covering a
  generic-argument array, an explicit or implicitly typed array creation, a declared
  array-typed local and a property-bound array alike; the edge's own `message` carries the
  array suffix.
  These are candidate type relationships: receiver registration, endpoint routing, test-host
  co-location and runtime delivery are not established. Behaviour precision and recall are
  unmeasured, and no gain in `uses-member` recall is claimed.
- **The message vocabulary is read from the repository, not shipped with the engine.** A
  repository's own one-type-argument handler registrations name the types it treats as
  handlers, and a registered type's generic bases are that repository's handler bases. Two
  closures spread what it says about itself: a base reached through a locally declared
  pass-through intermediate is still a handler base, and a method that hands its caller's
  message to a publish is itself a publish, followed exactly one hop. A small set of common
  handler shapes seeds the vocabulary so a repository that registers nothing still works, and
  `stats.bus_vocabulary_derived` reports whether the repository contributed -- `false` says the
  engine fell back to what it ships, which is a coverage gap rather than an absence of routes.
- **Inspectable handler evidence.** `base-arg` names a plain single generic message argument,
  `nested-base-arg` names the inner argument of a single-argument generic wrapper,
  `mediator-request` names a handler shape with a second type argument beside the message, and
  `property-arg` names a message a recognized handler binds on a property rather than on its
  base list. Calls may span lines, construct the message directly or inside a lambda, or name a
  message variable declared earlier. External or ambiguous message identities emit no hop. A
  publish written inside a mocking-library or repository-declared test-double setup/verify
  lambda -- an expression-tree-typed parameter, whether the call is written in extension or
  static form, and whether an optional trailing argument of that call is passed or left out --
  earns no hop either; a plain-delegate lambda around the same call is real dispatch and keeps
  its hop. A static-form call of a `this`-marked helper that leaves out an optional argument,
  while an equally admissible extension reading of the same call points its lambda at a
  parameter that is not expression-tree typed, is not yet told the two readings apart and keeps
  its hop -- a named gap.
- **`refs`, `read`, `impact` and `tests` traverse candidate hops**, with `--no-bus` to restore
  prior traversal and hub classification. The common report carries the publisher's `file:line`,
  query direction, resolved message, handler and declaring file, handler evidence, and how many
  handlers that message reaches in all; `bus-hop` remains the `why` word. That last count is
  what separates one route from one shared contract every publisher appears to reach, and it is
  computed from the edges rather than stored. Bus edges contribute distinct referring files to
  the existing file in-degree brake, and `--hub-max-indegree` controls further impact expansion
  through those files.
- **Every bus-hop row discloses it is a possible route.** Runtime routing is not verified:
  text, `--json` and `--compact` answers each state so, and the JSON row additionally carries
  the verification targets (publisher `file:line`, resolved message, handler identity and file,
  and which evidence is missing) as an additive object. A file reached only through a bus hop
  keeps that marker through later ordinary-reference hops within `impact`'s walk; a file also
  reached by an independent non-bus path keeps that path's stronger evidence instead.
- **Measured on the pinned C# corpus.** At MassTransit commit
  `855cf1752c94ca9498e0c45ce8d09fdc9e957dd6`, mapping the whole tree emits 23,687 candidate
  hops over 171 distinct messages, of which one shared test message accounts for 23,068 across
  79 handlers -- concentration every row now states rather than leaves to be inferred. An array
  of that same shared message is its own identity, reaching only the two handlers declared
  against the array: 4 hops from its 2 publish sites, no longer the 79 single-message handlers
  a rank-blind identity previously reached. Every non-bus edge kind and count is unchanged from
  the same commit's non-bus map. A registered type's own generic base is admitted into the
  handler vocabulary only when that type is not itself a sent message and actually receives,
  through a declared parameter or a bound property, a message the corpus does send -- a
  self-referential library base can no longer route a message to itself. These counts measure
  reach, not precision.
- **`map` consumes an admitted compiler-facts artifact at resolve time.** A same-context
  compiler fact determines the answer at its own occurrence, including where it contradicts a
  syntax-derived edge; the displaced syntax answer is preserved as a disagreement diagnostic
  (`stats.semantic` in `graph.json`), never retained as a graph edge. A compiler-verified
  occurrence the syntax extractor never emitted a reference for at all is projected as its own,
  separately provenanced population (`source: "semantic-discovered"`, never pooled with the
  per-reference overrides, and never moving the syntax-lane recall denominator). Freshness is
  whole-artifact: a changed git head or a dirty working tree since the artifact was captured
  invalidates every fact it carries, even for a consuming file that is itself byte-identical.
  `map --no-semantic` skips an admitted artifact even when one is present, always producing the
  exact syntax-only graph a build with no artifact admitted would. `audit --semantic` reports
  the enriched lane's two new tiers (`semantic`, `semantic-discovered`) alongside the syntax
  tiers, and a `lane` key naming which kind of graph was audited. Registered against a public
  ship/no-ship gate: see
  [`docs/benchmarks/results/2026-09-resolver-precision.md`](docs/benchmarks/results/2026-09-resolver-precision.md)'s
  "Run 7" section for the measured result, and
  [`docs/benchmarks/results/2026-09-25-release-0.8.0.md`](docs/benchmarks/results/2026-09-25-release-0.8.0.md)
  for the measurement of this release's tree. See [Compiler facts](docs/guides/compiler-facts.md).
- **CI exercises the enriched lane end to end.** The `semantic-audit` workflow job now also runs
  the oracle in `--emit compiler-facts` mode on the `csharp-semantic` fixture from the checkout
  root, so the artifact's `sourceSnapshot.headSha` is a real commit and its paths match the
  graph's. It admits the artifact, rebuilds the fixture's graph, and audits it against the same
  run's oracle output. The audit must report the enriched lane, the fixture's exact numbers of
  confirmed and discovered edges, and semantic precision 1.0, as pinned in
  `fixtures/csharp-semantic/expected-enriched.json`.

### Changed

- **Fragment cache generation v23.** The fragment shape gains publish-site facts, a handler's
  nested base type arguments, the single type arguments a type's properties wrap, one-argument
  handler registrations, publish sites named under a verb this engine does not ship, and the
  array-marked subset of a handler's own base names (so a single message and an array of that
  same message never collapse into the same base-list argument). The first run after upgrading
  remaps; every earlier generation, including v20, v21 and v22, is deleted on that run, because
  a reused earlier fragment would leave a supported hop unfound, or a single/array identity
  distinction lost, with nothing on disk to show for it. v22 was never present in a shipped
  release. The graph schema stays at 3: the `bus-hop` counter is absent rather than zero when a
  repository has no publish site, so a repository with no bus in it serializes byte for byte as
  it did before.
- **`src/audit.rs` is a thin module root over `src/audit/`** — the data model, filesystem load,
  pure scoring primitives, per-tier counters, rendering and the `--assert` threshold check, each in
  their own file, with the unit tests split the same way under `src/audit/tests/`. `audit --semantic`
  text and `--json` output are byte-identical before and after the split on the committed fixture.
- **`audit --semantic` reports a per-tier `unjudged` count.** A `semantic-discovered` edge whose
  originating compiler occurrence carries `shape: "identifier"` -- a bare field/property/event read
  the oracle's own walker records no case for at all -- is reported in its own `unjudged` count
  instead of scored as a false positive, since no oracle record could ever exist to judge it by.
  Read directly from this checkout's own admitted compiler-facts artifact (the occurrence's already-
  written `shape` field), never from `src/semantic/`. Every other tier's `unjudged` count is always
  0, and every other tier's figures, and the syntax-lane recall figures, are unaffected. See
  [`docs/benchmarks/results/2026-09-resolver-precision.md`](docs/benchmarks/results/2026-09-resolver-precision.md)'s
  "Run 7b" section.

### Fixed

- **The compiler-facts producer works under a registered MSBuild newer than its own packages.**
  The tool shipped its own older copies of three MSBuild assemblies (`Microsoft.Build.Tasks.Core`,
  `Microsoft.Build.Utilities.Core` and `Microsoft.NET.StringTools`), which a newer registered
  MSBuild bound ahead of its own: under a 10.0 SDK both the offline restore's project evaluation
  and `--emit context` aborted with a `MissingMethodException`. The installed SDK now supplies
  every MSBuild assembly. The offline restore also falls back to the SDK's own `dotnet msbuild`
  whenever its in-process evaluation fails for any reason, not only on a project error, so a
  binding failure there no longer ends the run. `--emit context` now also obtains the independent
  document inventory of a .NET Framework target on non-Windows machines, where it used to report
  that inventory unavailable.
- **Compiler-discovered edges are emitted in a deterministic order.** The enriched lane's own
  discovered-site projection used to walk its site keys in the underlying `HashMap`'s own
  iteration order, so `graph.json` was not guaranteed byte-identical across two separate `map`
  processes over the same admitted artifact whenever more than one discovered site existed. It now
  walks them in a fixed, sorted order.
- **Two confirmed same-type overloads at a compiler-discovered site now project as two distinct
  edges, not one ambiguous outcome.** A discovered site carries no caller-side argument count to
  narrow an overload set by (unlike an extractor-emitted reference), so two same-line, same-type,
  different-signature confirmed facts used to collapse together and project nothing. Each now
  projects with its own overload signature, the same way arity narrowing already lets an
  extractor-emitted reference resolve two same-line overloads independently; a survivor set naming
  more than one distinct type is unaffected and still projects nothing.
- **A repository-authored MSBuild import is identified by its own root-relative path, not just
  its bare file name.** `Directory.Build.props` and its kin previously normalized to their bare
  name alone; a same-named override file at a different repository depth (a root
  `Directory.Build.props` plus a subtree's own override, an ordinary MSBuild pattern) collided
  onto that one identity, and the freshness check -- which joins an import identity under the
  repository root before re-hashing it -- then compared one file's recorded hash against a
  different file's live content and read the whole artifact as stale on every run touching either
  one, even though nothing had changed. An installed .NET workload's manifest tree
  (`sdk-manifests/`) is now also recognized as SDK-owned, the same way the SDK's own `sdk/` tree
  already was, closing the matching gap where an unrecognized workload-manifest import fell
  through to the same bare-name identity and could never be located under the repository root at
  all -- both silently degraded the enriched lane to syntax-only rather than reporting an error.

## [0.7.0] - 2026-09-25

A compiler-evidence release: `compiler-facts` admits a versioned compiler-derived fact artifact
beside `graph.json`, the precise tier stops emitting `uses-member` edges C# name lookup cannot
produce, and the compiler-fact path's .NET target coverage is published per target.

### Added

- **`compiler-facts run|import|status`** admits a versioned, optional compiler-derived fact
  artifact -- either from a one-shot local engine run or from a build/CI-produced artifact --
  through one Rust admission path, publishing it atomically beside `graph.json`. `map` and every
  query verb never spawn the engine and never require the network; `status` reports
  `coverage: syntax-only` when no artifact has ever been admitted. A mismatched engine revision,
  contract version, requested profile, dependency fingerprint, compilation-context version or
  fingerprint, or source-snapshot identity is refused with a stable reason and writes nothing; a
  killed, timed-out, over-budget, truncated, malformed, or internally incoherent run leaves any
  previously admitted artifact byte-identical. A structurally valid artifact declaring incomplete
  coverage is still admitted, with its per-unit diagnostics, and is never reported as clean or
  complete. The default CLI distribution carries no managed runtime, targeting pack or engine
  assembly. The artifact additionally carries, by default, a per-reference **occurrence** fact for
  every invocation, member-access, conditional-member and identifier reference site the compiler
  can see: the caller's and bound target's complete identity, resolution state (confirmed,
  ambiguous, unresolved, inaccessible or dynamic) with the compiler's own candidate set, the
  binding compilation's identity and fingerprint, and per-document content identity -- produced by
  a walker independent of the existing declared-symbol facts, admitted through the same path under
  three new stable refusal reasons for a self-contradictory occurrence payload, and reported by
  `status` as its own coverage line. The artifact's header context summary is now recomputed from
  the real embedded build-context envelope's own per-compilation identity/fingerprint pairs rather
  than compared against a placeholder copy of itself. `engineRevision` moves to `"2"` and
  `artifactSchemaVersion` to `2`; `contractVersion` and the artifact filename are unchanged. See
  [Compiler facts](docs/guides/compiler-facts.md).
- **The Roslyn oracle emits a build-context envelope.** `tools/scout-semantic --emit context`
  writes one record per compilation identity (project path and name, requested target,
  configuration, platform), each carrying one of `complete`, `partial`, `unsupported`, `failed` or
  `excluded` with a machine-readable reason; the expected-versus-loaded document inventory with
  every difference classified (`missing`, `linked-outside-root`, `skipped-directory`,
  `out-of-scope`); raw restore/workspace/compiler diagnostics; generated and linked documents;
  SDK/MSBuild/compiler/engine versions; and a context fingerprint that moves with a reference,
  import, analyzer reference, generator input, build option, or dependency compilation's own
  fingerprint. A non-null compilation is never `complete` by itself, including when its own
  independent inventory could not be obtained at all -- `documents.inventoryAvailable` marks that
  case explicitly, even when a stronger reason already demotes the record's own `state`/`reason`.
  Additive: `refs.jsonl`, `units.jsonl`,
  `defs.jsonl` and the flow-tracer fact document stay byte-identical for unchanged inputs.
  Fixture: `fixtures/csharp-context/` and `fixtures/csharp-context-fingerprint/`. See
  `tools/scout-semantic/README.md#build-context-envelope`.
- **`refs`/`read`/`impact`/`tests --json` gain two additive top-level/row keys.** `freshness` is a
  new top-level object, always the true last key after `outcome`, reporting whether the index was
  built at the working tree's current HEAD (`"fresh"`, `"stale"` with both heads and the changed
  files, or `"unknown"` with a reason) -- the same signal the existing stderr freshness note
  already carries, now reachable by a programmatic consumer. `occurrenceIndex` is a new row key on
  `inbound`/`outbound` entries of `refs`/`read`, appended after `why` and omitted whenever a row has
  no same-table collision; it disambiguates two calls to one target that would otherwise serialize
  to byte-identical rows (for example two calls on one line), scoped to one table of one answer
  against one graph snapshot -- not a stable cross-run identity. No existing key is renamed, removed
  or reordered and `schema_version` is unchanged. See [`docs/reference/json-output.md`](docs/reference/json-output.md#freshness)
  and its [`occurrenceIndex`](docs/reference/json-output.md#occurrenceindex) section.
- **A new offline semantic-truth harness and fixture pack** (`devscout_rs::truth`, dev/test
  infrastructure only, no new CLI verb): a versioned case manifest schema with independently
  reviewed expectations, span-based occurrence identity, a nine-row fault-control battery, a
  compatibility-profile registry with a machine-readable capability matrix, freshness/
  transformation controls, and a committed truthful red baseline recording today's known
  analyzer misses rather than repairing them. Offline and deterministic: no `dotnet` and no
  network in the fast lane. A separate scheduled workflow probes the demonstrated profiles
  against a real compiler.
- **`audit --fp-sites <file>`** writes one JSON Lines row per false-positive `uses-member` edge --
  file, line, tier, class, the target the resolver bound, and the target(s) the oracle expected --
  for every tier, not only the precise one. Without the flag, `audit`'s text and `--json` output
  are unchanged.
- **`docs/dotnet-target-coverage.md`** publishes a per-target, per-project-system capability
  matrix for the compiler-fact path (`tools/scout-semantic`), separate from the language-level
  construct catalogue: modern .NET, .NET Standard and `netcoreapp3.1` profiles measured from
  their own exact compilation context, .NET Framework staged on its own track, and every
  planned, unavailable, excluded or unqualified target kept visible rather than omitted. Pinned
  by a new fixture tree (`fixtures/csharp-target-qualification/`), a new composition script
  (`tools/qualify-dotnet-targets.py`) and a dotnet-free test
  (`tests/dotnet_target_qualification.rs`); no `src/*.rs` change.
- **Eleven more measured rows in `docs/dotnet-target-coverage.md`:** .NET 10, built under its own
  pinned SDK band (10.0.302) from a `global.json` beside its project while every other row keeps
  the tree's 9.0.305 pin, and recording the MSBuild band the compiler-fact path registered; the
  .NET Framework 4.5, 4.5.2 and 4.6.1 reference-assembly profiles on the Framework track; and .NET
  Standard 1.0 through 1.6, whose rows record the `NETStandard.Library` version and the number of
  packages their restore resolved. Each compiles from its own single-target project; the Framework
  and pre-2.1 Standard rows record the same boundary non-bind (`CS1501`) as the existing ones, and
  the .NET 10 row binds it. None has a held-out row yet; the document lists that gap. CI installs
  the 10.0.302 band immediately before the matrix regeneration step, so every earlier .NET step
  keeps resolving 9.0.305. The coverage test's support-sentence sweep now matches whole
  target monikers, so a measured target no longer masks, or trips on, a longer or shorter one that
  is still unmeasured.
- Offline extension-impact qualification harness reporting candidate-edge and complete-answer
  precision against compiler-reference evidence. It preserves production traversal and records
  failed admission gates without promoting evidence tiers.

### Changed

- **`--tfm` is now repeatable**, with each requested target its own compilation identity and no
  silent substitution when a project does not declare it; with no `--tfm`, selection is
  deterministic (ordinal-least declared target) rather than dependent on workspace enumeration
  order. This reaches `--emit oracle`/`flowtrace-facts` too: a project whose sole declared target
  does not match `--tfm` is now dropped from the run rather than kept regardless of the request,
  and a run where every project is dropped exits 3 where it previously produced facts.
- **`--strict` gains an artifact-level check for `--emit context`**: exit 2 unless every
  non-`excluded` record is `complete`, alongside its existing checks. A project a `--projects`
  filter leaves out is `excluded`, not `failed`, so it never trips this check.

### Fixed

- **The precise tier no longer emits `uses-member` edges C# name lookup cannot produce.** Five
  resolver shapes are refused instead of guessed: a bare qualifier resolved only by graph-wide
  simple-name uniqueness; a receiver whose written type-argument count has no in-tree match; a
  property whose name shadows a same-named type; a lambda or local-function argument whose
  parameter count cannot fit the candidate's delegate; and a nullable value-type receiver's own
  `Value`, `HasValue` or `GetValueOrDefault`. On the pinned MassTransit benchmark, precise false
  positives against external targets and structural impossibilities drop to 0 (precision 0.989 to
  0.993). The fragment cache moves to `fragments-v20.json`, so an existing cache re-extracts once.
- **A member qualifier's own type-argument count now decides which same-named type it binds.**
  `Foo.M()` and `Foo<T>.M()` on the same line each resolve to their own definition -- a bare
  qualifier to the zero-type-parameter sibling, a one-argument qualifier to the one-type-parameter
  sibling -- regardless of which definition the index visits first. A qualifier whose written
  argument count has no matching definition (for example a two-argument qualifier where only a
  bare and a one-argument sibling exist) earns no precise edge instead of binding a same-named
  guess. The fragment cache moves to `fragments-v21.json`, so an existing cache re-extracts once.

## [0.6.0] - 2026-09-09

A reach release: the graph learns which implementation a registered service resolves to, and
`impact` can name files that live in another repository.

### Added

- **`import-edges <file> --repo <id>`** loads a versioned cross-repo edge export into an
  auxiliary artifact beside `graph.json`, never touching the graph schema. `impact` then names
  files reached only through an imported edge -- directly, or composed across a fileless message
  node -- each row carrying `why: "imported-edge"`, the foreign repo id, and the export's
  provenance id. Imported rows are capped and counted apart from the native ones, so an import
  can never evict a native row. An invalid export is refused with exit 1 naming the offending
  value, leaving any pre-existing artifact untouched; a successful import replaces the prior set
  wholesale. `impact --no-imports` skips it, and a repo with no import configured answers exactly
  as it did before.
- **Dispatch edges from dependency-injection registrations.** A registration whose method name
  begins `Add` or `TryAdd`, ends `Singleton`, `Scoped` or `Transient` and carries exactly two type
  arguments becomes an `implements` edge from the implementation to the service type, provided
  both resolve to exactly one in-graph definition. A member-level `implements` edge is then added
  per matching interface method and an `overrides` edge per method carrying the literal `override`
  modifier, scoped to the implementation types a registration named. Arity ties emit nothing,
  matching the resolver's never-guess rule, and every other registration spelling -- keyed and
  named included -- records nothing extra by construction. A repository with no registrations
  gains no edges.
- **`refs`, `read`, `impact` and `tests` traverse the two new edge kinds**, joining inbound and
  outbound the same way `inherits` already does, with `--no-dispatch` on each verb to answer
  without them.

### Changed

- **Graph schema 3.** `Edge::Implements` and `Edge::Overrides` with their `edges_by_kind`
  counters, `FragDef.override_methods` and `Fragment.registrations` in the fragment shape, and
  the fragment cache generation at v19 -- the first run after upgrading remaps.
- **`src/cli.rs`, `src/render.rs` and `src/graph.rs` are thin module roots** over `src/cli/`
  (one module per verb), `src/render/` (one module per verb) and `src/graph/` (one module per
  artifact layer). Every public item kept the path it had, and `graph.json` is byte-identical
  across each split on the pinned corpus.
- The comment-hygiene scanner is the vendored canonical one, its `--selfcheck` runs in CI
  alongside the scans, plan labels are a rejected comment class, and hook mode scopes itself the
  way `--scan` does.

## [0.5.0] - 2026-09-07

A code-organisation release: the three largest modules are split by concern, CI now holds
them that way, and every `--json` answer says what it is and why.

### Added

- **`ARCHITECTURE.md`.** One row per module under `src/` giving its responsibility and its
  invariants, plus a table saying where a change goes. `tools/check-architecture.sh` fails
  when a module has no row or a row names a module that no longer exists, and CI runs it.
- **Size and complexity gates.** `too_many_lines` (100) and `cognitive_complexity` (25) are
  denied; each existing offender carries one `#[allow(..., reason = "...")]` attribute.
  `tools/size-ratchet.toml` caps how many of those attributes the tree may hold and caps
  every file under `src/` at 800 lines, except the large files it names individually at
  their current length. Those numbers only shrink. `tools/check-size-and-complexity.sh`
  enforces the ratchet, a self-test proves the checker rejects what it should, and CI runs
  both alongside `cargo clippy --lib --bins --locked`.
- **Member seeds on `refs`, `read`, `impact` and `tests`.** A bare member name,
  `Type.Member` or `Namespace.Type.Member` resolves as a seed once the type-resolution
  ladder has found nothing, so no answer a type seed used to give changes. A member carried
  by more than one type lists one candidate row per declaring type with its file and line,
  never a bare type list, and `--pick N` narrows to the nth. When the did-you-mean pass
  holds an exact-name match it is answered instead of advising a text search.
- **`outcome` on every `--json` answer.** One of `hit`, `zero-hit`, `ambiguous` or
  `fallback-advised`, from a closed vocabulary, so a caller can count dead ends without
  parsing prose.
- **`schema_version` and per-row `why` on every `--json` answer.** `schema_version` is the
  first key of the object; every hit row carries a `why` naming the rule or tier that
  produced it, drawn from a closed vocabulary and derived from the edge the row came from.
  [`docs/answer-contract.md`](docs/answer-contract.md) documents the shape per verb with a
  worked example, and says that consumers should ignore unknown keys.
- **Query telemetry behind `SCOUT_TELEMETRY=1`.** Each answered `find`/`refs`/`read`/`impact`/
  `tests` invocation appends one JSON line to `scout/log/queries.jsonl` under the artifact
  directory: timestamp, record schema version, verb, seed, outcome, elapsed milliseconds,
  result bytes and candidate count. A usage error or a seed with no resolved repository or
  graph logs nothing; without the variable nothing is created; an unwritable log changes
  neither exit code nor output. Telemetry is opt-in: export `SCOUT_TELEMETRY=1` in the shell
  that runs the query verbs. The agent hooks never run those verbs, so `devscout init` does
  not set the variable for them.

### Changed

- **`src/resolve.rs` is a thin module root over `src/resolve/`** — the def index, arity
  admission, member checks, file scope, receiver typing, the ladder, edge construction and
  graph assembly, with the unit tests in per-topic files under `src/resolve/tests/`.
- **`src/extract.rs` is a thin module root over `src/extract/`** — split by construct
  family: types, type definitions, members, references, receivers, lambdas, qualifiers, the
  extraction walk, the dump and JSON plumbing, and the TypeScript-family extraction.
- **`src/query.rs` is a thin module root over `src/query/`** — one module per verb (`find`,
  `refs`, `read`, `impact`, and `coverage` for the tests-reaching-a-symbol verb) over the
  shared substrate they sit on: the graph index, symbol resolution, ranking, hub-file
  classification and the ordered collections.
- Every public item kept the path it had before the three splits, and `graph.json` is
  byte-identical before and after each of them on the pinned corpus and on both audited
  fixtures.
- **An ambiguous member seed answers with candidate rows.** It previously rendered exactly
  like an ambiguous type name and ignored `--json`; it now lists the member candidates and
  emits JSON like every other answer.

## [0.4.0] - 2026-09-06

### Added

- **`uses-member` edges carry `tier` and `member`.** `tier` names which heuristic tier
  emitted the edge (`ext` for extension-method lookup, `guess` for the scored name match);
  `member` names the member the reference reads or calls, on precise and heuristic edges
  alike. Only `tier` (and `heuristic`) are omitted on a precise edge; `member` is written
  there too, which is why a precise edge's bytes change under this schema bump as well.
- **`--no-guess` on `refs`, `read`, `impact`, and `tests`.** Admits only the `ext` tier's
  edges into the query surface, dropping every scored-tier guess; compact output marks the
  two tiers `x` and `h`.
- **`stats.heuristic_by_tier` splits `heuristic_edge_count` by tier.** Always written, with
  `ext` and `guess` counts that sum to `heuristic_edge_count`.
- **A lightweight `.csproj` project model.** Hand-scans `ProjectReference`,
  `Microsoft.NET.Test.Sdk`, and `IsTestProject` out of `.csproj` and `Directory.Build.props`
  files, with no MSBuild evaluation. Discovered projects are persisted as `units` in
  `graph.json` and mirrored in a `project-units.json` sidecar; editing a `.csproj` is now
  itself a rebuild trigger.
- **`tests` lists harness files that live in test projects.** When a project model exists, a
  file in a test project that reaches the symbol is listed even without attribute-marked test
  methods, suffixed `(test project)` in text and carrying `"via":"project"` (appended last) in
  JSON, so a harness reference stays distinguishable from a discovered test; `impact`'s
  `testsAffected` counts both.
- **`this.` and `base.` member accesses resolve to the enclosing type and its bases.**
  `this.Name` types the reference as the enclosing type, including its own type arguments
  when the type is generic, and emits no reference outside any type; `base.Name` starts the
  member lookup at the enclosing type's in-graph bases, in declaration order, walking each
  base's own inheritance chain, and never considers the enclosing type itself — no in-graph
  base declaring the member resolves as an ordinary external receiver, not a guess.
- **`?.` bindings emit the same reference a plain member access would.** `a?.B` resolves
  exactly as `a.B` does, at the conditional-access expression's own line.
- **Local type facts see through `await`, casts, patterns, and typed `out` parameters.**
  `var x = await Repo.LoadAsync();`, `var x = (T)e;`, an `is`/switch pattern designation, and
  `out T x` all record a type fact for the introduced name; `out var x` still records none.
- **A local assigned from an awaited call resolves through one `Task<T>`/`ValueTask<T>`
  unwrap.** `var o = await Repo.LoadAsync();` types `o` from the unwrapped return type
  rather than the raw `Task<...>`, and a `Task<Task<T>>` return is unwrapped only once.
- **One-hop call-chain tails are typed.** A member access on the result of `a.B()` carries
  the inner call as its receiver and resolves through one method-return hop; a qualifier
  that is itself a chain (`.D` on `a.B().C()`) or a null-conditional chain (`a?.B()?.C`)
  emits no typed reference.
- **A collection receiver types its first lambda argument's sole parameter.** A
  single-parameter lambda passed as the first argument to a call on an array- or
  single-type-argument-generic-typed identifier gets that parameter typed as the element
  type; two-argument generics, multi-parameter lambdas, and later arguments get none. The
  rule reads the receiver's shape only, so a single-type-argument wrapper such as `Task<T>`
  or `Lazy<T>` types the parameter as its type argument too.
- **Field types cross files.** A bare-identifier receiver with no in-file type fact is typed
  from the enclosing type's own field and property declarations, merged across every file a
  partial type spans, then from the same tables on each in-graph base in declaration order;
  an in-file local or parameter of the same name always wins over this fallback.
- **Method arities are recorded per overload.** Each declared method records the
  parameter-count range every overload accepts, an unbounded `params` overload left
  open-ended and optional parameters lowering the minimum.
- **An untyped lambda parameter is typed from the callee's delegate parameter.** Every
  method records its parameter types per overload (`methodParams`, type parameters as `*`,
  an extension method's receiver as `this <type>`; a `delegate` declaration records its own
  parameters under `Invoke`). A lambda passed straight to `ident.M(...)`, `this.M(...)` or a
  bare `M(...)` whose parameter carries no annotation and earns no collection element fact
  records the call it sits in; when that callee is an in-graph method (or extension) whose
  parameter at the lambda's position is `Action<..>`, `Func<..>`, `Predicate<T>`,
  `Expression<>` of one of those, or an in-graph `delegate`, each lambda parameter is typed
  positionally from that delegate and its member accesses resolve as typed-receiver edges.
  Overloads that can take the lambda must agree on the type, a `*` yields nothing, and the
  callee's receiver is read one hop deep only (an in-file fact, a static class name, or the
  enclosing type), and a parameter name that two lambdas in one member bind to different
  callees records no slot. The fragment cache moves to v18.
- **TypeScript aliases follow the nearest `tsconfig.json`.** A bare specifier resolves through
  the `paths`/`baseUrl` chain of the closest ancestor `tsconfig.json` of the importing file,
  so an app-level `@/*` inside a workspace resolves instead of falling out external; the
  repo-root `tsconfig.json`/`tsconfig.base.json` chain stays the fallback for files under no
  nested config or under one that declares neither option. A nested `paths` is a
  whole-property override, TypeScript's own rule: it also turns the root's aliases off for
  the files beneath it, so a specifier those files resolved through the root before now
  resolves only if the nested `paths` names it.
- **Chained TypeScript barrels are followed.** A name pulled through up to eight nested
  `index.ts` re-exports (`export * from`, `export { X } from`) resolves to its declaring
  file; a visited set makes a re-export cycle terminate. The barrel-followed `import` edge
  still carries `via`, and a `jsx-use` or `call` edge bound through a two-hop barrel now
  lands on the declaring file instead of resolving to nothing. `refs`/`impact` still fold
  none of the TS edge kinds (README, Limitations). Pinned by `fixtures/ts-resolution/`.
- **The Roslyn oracle emits the flow tracer's fact set.** `tools/scout-semantic --emit
  flowtrace-facts` writes one provider document per solution (`schemaVersion`, `producer`,
  `version`, `repo`, `kind: backend`, compilation identity, git identity, then `facts`) in the
  flow tracer's published fact schema: `message_class`, `consume`, `publish`, `ctor_field`,
  `di_binding`, `iface_impl`, `route` and `method_span`, with message, parameter and
  handler-lambda types resolved by the compilation rather than read off the text -- a primary
  constructor's consumer, a publish of a local variable, and a minimal-API lambda's parameters
  all become facts. Output is sorted and byte-reproducible; every fact is checked against the
  embedded required-field table before it is written, and `--strict` fails the run on any
  site the walk recognised but could not resolve. A package-free fixture solution under
  `fixtures/csharp-flowtrace/` pins the output as a committed snapshot that CI diffs, and
  `tests/flowtrace_facts.rs` pins its shape without a .NET toolchain.
- **A C# construct catalogue with a pinned fixture.** `docs/csharp-coverage.md` enumerates
  every C# construct the extractor meets, with its grammar node kinds, a syntactic verdict,
  an obligation (`must` / `may` / `must-not` produce a fact), the status measured at this
  release, and the fixture that shows it; `fixtures/csharp-syntax/` holds one compilable file
  per construct group, and `tests/csharp_syntax_matrix.rs` fails when a fixture stops parsing
  clean under the pinned grammar, when a producing `must` row stops producing, or when the
  fixture set and the catalogue drift apart. The grammar's three known gaps (C# 14
  `extension` blocks, `allows ref struct`, list-pattern slice designations) are recorded with
  the crate version.

### Changed

- **Graph schema bumped to 2.** `tier`, `member`, and `units` are the new keys; a v1
  graph.json is rebuilt automatically on the next `map` rather than read as-is.
- **`global using` scopes to the project that declared it when a project model exists.**
  Without a model every `global using` is still repo-wide, unchanged.
- **Tier (f) admits an extension class from an enclosing namespace, not just an imported
  one.** `App.Ext` is now visible from `App.Ext.Deep` with no `using` at all, matching C#'s
  own namespace-visibility rule.
- **A same-named ambiguity is narrowed by project reachability.** When a project model
  exists and the ladder finds two same-named defs, candidates the reference site's project
  cannot reach are removed before the precise tiers judge the result: one survivor resolves
  precisely, none behaves like an external name, two or more stay ambiguous with the shorter
  list. Without a model nothing changes.
- **Fragment cache moves to v16.** The next `map` after upgrading reparses every file once;
  the superseded v15 cache files are removed.
- **Typed-receiver lookups walk the receiver's base closure, class bases before any
  interface, in declaration order.** A class-typed receiver never binds to an interface
  declaration at any depth; the extension tier tries every in-graph base, and its raw base
  names, as a lookup key when the receiver's own type misses, in no promised order.
- **Non-public base members are visible to `this.` and `base.` lookups only.** The scored
  tier keeps vouching only through publicly declared members, so a guess never resolves
  through a private one.

### Fixed

- **A call no longer vouches through a property or field.** `entity.Property(x => x.Id)` has
  no overload-resolution path to a property or field of that name, so the scored tier no
  longer lets one stand in as evidence for a call shape it cannot answer.
- **A scored guess under an external receiver must be nominally assignable to it.** A
  candidate the receiver's type could never actually be is now a disproved guess rather than
  a weak one.
- **A scored guess never crosses into a project the reference site cannot reach, or into a
  test project from non-test code.** When a project model exists, both are now structural
  refusals rather than name-only guesses.
- **A `base.` call no longer binds to an interface base.** The base walk skips interface
  bases entirely — an interface declares a contract, not a target.
- **A call whose argument count no overload admits no longer binds to the same-named
  instance member.** It falls through to the extension tier instead, exactly as it would
  for a member the receiver does not declare at all.
- **A receiver typed by a call whose method-return hop fails no longer enters the guess
  pool.** When the hop yields no in-graph receiver type — for a chain tail, or for a local
  assigned from a call or an awaited call — the reference is finished as external instead of
  falling into the extension tier with an unknown receiver or the scored tier's
  name-uniqueness pool.
- **Extension-method generic unification checks the matched base's own type arguments.**
  When the extension tier reaches its lookup key through the receiver's base closure,
  unification runs against that base's declared type arguments rather than the receiver's
  own, so an extension declared on an implemented interface binds for a generic enclosing
  type.
- **Overloads a partial type declares across files all admit their calls.** Method arities
  merge as a union per name across every file the type spans, so `this.Run(1)` with `Run()`
  in one file and `Run(int)` in another binds instead of falling through.
- **A `base.` lookup on a cyclic hierarchy never binds the enclosing type.** The
  base-closure walk starts with its own type already marked as seen.
- **A field or property type declared in another file resolves in that file's context.**
  A base or sibling-partial field typed `Alpha` resolves `Alpha` with the declaring file's
  `using`s and namespace, not the reading file's, so a same-named type visible only from
  the reading file no longer takes the edge.
- **Every catch, query-range and lambda binding shadows the cross-file field fallback.** A
  `catch (T e)` designation records a type fact; a query range variable and a lambda
  parameter the element rule does not type take the name untyped, so a same-named field on
  the enclosing type or a base can no longer type them.
- **A `base.`-qualified chain head hops through the base's method return.**
  `base.Make().Validate()` types the tail from the first in-graph base declaring `Make`,
  never from the enclosing type's own same-named member.
- **The scored tier no longer re-admits an extension the extension tier declined on arity.**
  An extension of the receiver's exact type is still a valid guess when only its namespace
  was not imported at the site, but a call whose argument count the extension cannot take
  has no binding under any import and is refused there too.
- **A receiver written `Foo<X>` binds the generic `Foo<T>`, not whichever of `Foo` and `Foo<T>`
  was indexed first.** The two share one id, so a declared receiver type now resolves with the
  argument count it was written with and each base-list entry with the count its generic-argument
  record carries; the precise tier, the base walks it shares with the extension veto, and the
  veto itself read the sibling the language names. A name with no def at that count anywhere
  keeps the arity-blind answer; a name shared by fewer than two defs costs one lookup as before.
- **A static qualifier walks through nested types before its next segment is read as a
  member.** `Outer.Inner.Leaf.Value`, with or without a namespace prefix on `Outer`, binds
  the qualifier one nested-type segment at a time from the shortest head that names a type
  and emits a single precise edge to `Outer+Inner+Leaf` with `member` `Value`. The shorter
  windows of the same chain no longer emit an edge that names a nested type as if it were a
  member of its container, which is where a namespace-qualified head used to produce a
  precise edge to `Outer` with member `Inner`, and a nested type whose simple name repeats
  across containers now binds to the container the qualifier names instead of dropping out
  as ambiguous.
- **A fully qualified name never falls back to its bare last segment.** A dotted reference
  whose exact-qualified lookup fails matches only a def whose full path (a nested type's `+`
  read as `.`) ends with the text as written, and is external otherwise:
  `RabbitMQ.Client.ExchangeType.Fanout` no longer binds to an in-tree `ExchangeType`,
  `System.Text.Json.JsonSerializer.Serialize(x)` no longer binds to an in-tree
  `JsonSerializer`, and `expr.Member.Name` no longer binds to a nested type named `Member`.
  `Outer.Inner` still reaches `Outer+Inner`, `Box<string>.Slot` and `global::App.Widget` are
  read as the def paths they spell, `Derived.Item` reaches an `Item` declared inside a base of
  `Derived`, and a `using` alias at the head of a dotted name is rewritten to its target and
  looked up exactly.
- **A qualified or generic static qualifier binds the base that declares the member.**
  `Ns.Derived.Create()` and `Derived<int>.Create()` used to bind `Derived` on type certainty
  alone; the named type, then its in-graph base closure, is asked which def declares the
  member first, and the certainty answer is kept only when neither does.
- **An interface-typed receiver binds the base interface that declares the member.**
  `IExtended : IContract`, `ext.Fulfil()` resolves precisely to `IContract` instead of
  emitting nothing; a class-typed receiver still never binds an interface declaration at any
  depth. Pinned by a second fixture solution, `fixtures/csharp-direction`, with its own
  committed oracle snapshot, `expected.json`, and CI oracle diff, whose precise tier scores
  every inheritance-direction shape the compiler decides along an `inherits` edge and names
  its four remaining false positives (see the README's limitations).
- **Declarations and references inside an inactive preprocessor arm are no longer indexed.**
  The C# extractor now evaluates `#if`/`#elif`/`#else`/`#endif` before parsing, with the
  no-build symbol model: no symbol is predefined (`DEBUG` and `TRACE` included), `#define`
  and `#undef` inside the file are honored, and every other symbol is false, so at most one
  arm of every group reaches the parser. A file whose namespace or a member header is
  chosen by a symbol previously yielded every type under a doubled namespace and every ref
  from both arms; it now yields each once, under the arm the compiler keeps. Inactive lines
  are blanked in place, so line numbers and byte offsets of the surviving text are
  unchanged. `#region`, `#pragma`, `#nullable`, and `#line` are left alone, and a
  directive-looking line inside a block comment, a verbatim string, or a raw string literal
  is not a directive. The fragment cache generation moves to v17 so every file reparses
  once on the next `map`.

### Benchmarks

- **Resolver-precision benchmark against a compiler oracle.** `tools/scout-semantic` (a C# console
  project, not part of the crate build) emits one record per member reference from a compiled
  solution; the plumbing verb `devscout audit --semantic <refs.jsonl>` scores `uses-member`
  edges per tier for precision, recall, external-receiver leaks and cross-project impossibility,
  with `--assert` thresholds for CI. `fixtures/csharp-semantic` pins the defect shapes; baseline
  figures on the pinned MassTransit corpus are in `docs/benchmarks/results/2026-09-resolver-precision.md`.
- **Recall and precision on the pinned MassTransit corpus, Run 1 through Run 4.** `this.`
  receivers climb from 0.000 to 0.944 recall and `base.` receivers from 0.000 to 0.978;
  overall recall rises 0.479 → 0.546 (precise-only 0.379 → 0.431, precise+ext
  0.394 → 0.467). Extension-tier precision rises 0.809 → 0.906 on more than double the
  edges (1083 → 2272); precise precision holds near-flat at 0.969 → 0.972 and guess
  precision at 0.502 → 0.512; leaked external sites fall 1098 → 1011. Run 4 misses four of
  its registered recall predictions (`call` 0.060 against ≥ 0.20, `ident` 0.628 against
  ≥ 0.63, all 0.546 against ≥ 0.55, precise+ext 0.467 against ≥ 0.47). Full per-tier
  figures, every prediction with its verdict, and the open enrichment decision are in
  `docs/benchmarks/results/2026-09-resolver-precision.md`.
- **A stale fragment cache silently read this branch's additive tables as empty.** A corpus
  run measured against a fragment cache built by an earlier commit on this branch dropped
  every table it had not yet cached — base closures, arities, field types, and the rest —
  understating both recall and precision; every corpus run now wipes the indexer state
  before mapping.

### Docs

- **Design for the compiler-backed enrichment layer.** `docs/design/compiler-enrichment.md`
  settles, before any code, how a cached, out-of-process consumer of the oracle's per-site
  records would enter `resolve`: the record contract, the `semantic-v1.json` cache and its
  per-file content-hash validity, the `source: "semantic"` edge tag in the slot schema 2
  reserved, how `audit --semantic` keeps the syntax-tier figures comparable, the sidecar
  invocation model, and the gate the layer must clear to ship. No behaviour changes.

## [0.3.0] - 2026-08-27

### Added

- **`read` verb.** Serves a symbol's declaration span plus its inbound callers — with real
  declaration spans for TypeScript as well as C#. The first read of an indexed file offers
  the nearest symbol, including ranged reads, which offer the symbol nearest the requested
  range. References originating inside the target's own declaration span are excluded from
  inbound results and counts.
- **`find` ranking.** Results are ranked by precise inbound reference counts. References
  originating in the same file are excluded from a file's inbound count.

### Changed

- **`init` language census reports three honest tiers**: C# fully supported;
  TS/TSX/JS indexed and graphed with narrower edge coverage; other counted extensions
  present, not indexed. The census no longer understates TypeScript.
- **The content-database default resolves at runtime** from the home directory, matching
  the registry path's resolution, instead of a compile-time manifest path. The
  `SCOUT_CONTENT_DB` override is unchanged.
- Crate-wide rustfmt adoption and a full public-API rustdoc pass
  (`missing_docs = "warn"` at zero warnings). CI enforces formatting from this release on.

### Fixed

- **Bare names no longer bind to nested types the reference site cannot name.** A bare,
  undotted type name reaches a nested type only when the reference site sits inside the
  nesting chain or inherits the enclosing type; other bare references fall through as
  unresolved or external instead of emitting a false precise edge that `impact` then
  widens through (for example, a bare `Claim` under `using System.Security.Claims`
  binding to an unrelated nested test class).
- **Generic arity is matched exactly during resolution.** A type reference resolves only
  to definitions with a matching type-parameter count; arity-overloaded siblings stay
  distinct, and mismatched references fall through as unresolved or external instead of
  binding a wrong same-named definition. The fragment cache generation is bumped, so the
  first run after upgrading re-extracts.

### Benchmarks

- The published scorecard was measured on 0.2.0 and is not re-measured in this release.
  `find` output ordering and reference resolution changed in 0.3.0, so those numbers
  describe 0.2.0 until the next benchmark round.

## [0.2.0] - 2026-08-25

Both entries are fixes for defects the first benchmark round found in this tool and
recorded against itself (`docs/benchmarks/results/2026-08.md`, "Defects this run found
in its own method"). Both change observable output, hence the minor bump.

### Changed

- **`find`: every manifest-pool row now carries a line.** Rows were `path: purpose`,
  with no line, so a follow-up could not open the hit at a position and the row failed
  the benchmark's follow-up-reach rule. They are now `path:line: purpose`, where `line`
  is that file's own first declaration in the name index, falling back to line 1 for a
  file the index carries no declared symbol for at all. The declaration block above it
  is unchanged.

### Fixed

- **`refs --all` now lifts the inbound cap too, not just the outbound one.** `--all`
  lifted `OUTBOUND_CAP` only, so a `refs` answer truncated at `INBOUND_CAP = 30` had no
  flag that could recover it and silently returned fewer referring files than the graph
  held. `--all` now lifts both caps. Output without `--all` is unchanged.

## [0.1.0]

Initial public release: `map`, `find`, `refs`, `impact`, `init`, `stats`, `clear` over a
C# and TypeScript/JavaScript index.
