# csharp-enrichment fixture

Used by `tests/semantic_enrichment.rs`. A small, hand-authored C# tree that
gives the compiler-fact enrichment consumer (`src/semantic/`) three distinct
kinds of site to react to, all driven from a hand-built admitted-artifact
JSON the test constructs itself -- no `dotnet` build, no oracle run, and no
producer output is ever involved.

- `src/Config/Alpha.cs` and `src/Config/Beta.cs` declare the same short type
  name (`Config`) with the same static member (`Load`) in two different
  namespaces, both brought into scope by `using` in `src/Caller.cs`. An
  unqualified `Config.Load()` call is genuinely ambiguous to devscout's own
  syntax resolver, which pushes one scored ("guess") edge per candidate --
  this is the fixture's `THREE_TIER_FIXTURE` precedent in
  `src/resolve/tests.rs`, reused here for a distinct purpose: a real,
  reproducible case of syntax binding to more than one target at once, so a
  compiler fact can be shown confirming exactly one of them and displacing
  the rest.
- `src/Caller.cs`'s `SameContextOverride` and `NegativeCollision` methods
  both call the ambiguous `Config.Load()`, at two different lines, so the
  test can admit a *confirmed* compiler fact for one call and an
  *ambiguous* one for the other -- proving the override fires at the first
  and is a correct no-op at the second, from the same fixture tree.
- `src/Factory.cs`'s generic `Get<T>()` plus `src/Caller.cs`'s
  `LocalCallResult` method give a reference (`thing.Render()`) whose
  receiver's real type only exists through generic-method type inference --
  a local call result, the kind of site the syntax extractor alone still
  misses.
- `src/Handler.cs`'s `Handler<T>` (an abstract generic base declaring
  `Get()`, returning `T`) and its non-generic derived `WidgetHandler :
  Handler<BetaWidget>`, paired with `Caller.cs`'s `InheritedGenericCallback`
  method, give a reference whose receiver type comes from a generic BASE
  class's own type argument substituted at a non-generic DERIVED class --
  distinct from the generic-method-return case above because the type
  argument is never spelled out at the call site itself (`handler.Get()`
  carries no `<...>` at all).
- `src/Caller.cs`'s `NestedTypedLambda` method reuses the generic-method-
  return shape, but the `.Render()` call it targets sits inside a SECOND
  lambda nested inside the one that declares the local -- proving the
  override reaches a reference two lambda scopes deep, not only one at a
  method's own top level.
- `src/Container.cs`'s generic indexer (`Container<T>`'s `this[int]`,
  returning `T`) paired with `Caller.cs`'s `TypedIndexerResult` method give a
  reference whose receiver type is a generic indexer's own return type --
  a fourth, distinct route by which a receiver's real type is invisible to
  the syntax ladder.
- `src/Registry.cs`'s `Registry<T>` (an abstract generic base declaring the
  PROPERTY `Current`, returning `T`) and its non-generic derived
  `WidgetRegistry : Registry<BetaWidget>`, paired with `Caller.cs`'s
  `QualifiedPropertyAccess` method (`registry.Current.Render()`), give the
  "qualified property access" receiver category: the same generic-
  substitution-across-inheritance blind spot `InheritedGenericCallback`
  exploits for a method, but here the receiver of `.Render()` is itself a
  QUALIFIED (dotted) property-access expression, never stored in a bare
  local first, unlike every case above. `Config.Load()` above is a
  type-qualified STATIC METHOD call, already resolvable (if ambiguously) by
  the syntax ladder via `using`, and is a distinct case from this one.
  Reached through a property hop rather than a recorded call, this site's
  own syntax-only build is baseline-AMBIGUOUS rather than unbound (the
  syntax ladder's member-name-uniqueness fallback, silenced for every other
  category above by their own call-hop fact, still fires here), so
  `src/Widgets/GammaWidget.cs` gives it a second, wrong `Render()` declarer
  to guess between -- the confirmed fact then displaces BOTH guesses,
  proving the same override shape `Config.Load()` proves, for this category.
- `src/Caller.cs`'s `SameLineDistinctFacts` method puts two DIFFERENT member
  references (`thing.Render()` and `other.Paint()`, `BetaWidget` now
  declaring both) on one physical source line, proving the compiler-fact
  consumer's compatibility join key is `(file, startLine, member)`, never
  `(file, startLine)` alone -- the two facts must resolve independently, not
  be conflated because they share a line.
- `src/Options.cs`'s two overloads of `Configure` (`Configure()` and
  `Configure(bool)`), paired with `Caller.cs`'s `OverloadedSameLine` method
  (`options.Configure(); options.Configure(true);`, both on one physical
  line), give the SAME member name twice at the SAME compatibility key --
  distinct from `SameLineDistinctFacts` above, which uses two different
  member names. Each admitted fact carries its own `overloadSignature`; the
  consumer's `SemanticLayer::lookup` uses it, matched against each call's
  own argument count, to keep the two overloads as distinct facts rather
  than collapsing them into one ambiguous outcome.
- `src/Tie.cs`'s single-overload `Resolve(bool)`, paired with `Caller.cs`'s
  `EqualArityOverloadTie` method (`tie.Resolve(true);`, its own single
  reference), gives an extractor-emitted site where two admitted facts name
  the SAME declaring type and member but different overload signatures of
  the SAME argument count, so the call's own argument count cannot narrow
  the pair to one. It is a dedicated type rather than a third call through
  `Options` so this case's own `refs Resolve --json` surface stays isolated
  from `OverloadedSameLine`'s `refs Configure --json` count above. Unlike
  `OverloadedSameLine`, where two DIFFERENT calls at one shared key each
  narrow to their own single candidate, this is one call whose own arity
  ties both candidates at once: `SemanticLayer::lookup` reports
  `ConfirmedMany`, and `precedence::decide` only ever matches
  `LookupOutcome::Confirmed`, so no override fires and the syntax ladder's
  own precise edge (bound off the `Tie`-typed local) survives untouched --
  the same fall-through an outright ambiguous fact gets, for a different
  reason (a real tie the compiler facts cannot break, rather than a fact
  the layer itself could not confirm).

Every occurrence fact the test admits is constructed by hand in
`tests/semantic_enrichment.rs` itself (line numbers are read back from a
syntax-only `map` run first, never hand-guessed), so a fixture's expected
state is authored independently of any producer's output.
