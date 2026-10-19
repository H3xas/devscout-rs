# bus-reserved-param-keys fixtures

A shoal-survey estate with invented, neutral names. No name here comes from a
bus that the engine ships handling for, or from another bus fixture.

The extractor records some facts in `methodParams` under reserved keys:
`@result:<name>`, `@property:<name>` and `@field:<name>` hold a member type,
and `@variant`, `@implicit-conversion` and `@interpolated-string-handler`
mark the type. None of these entries is a declared parameter. The admission
rule needs a declared parameter or a bound property that receives a sent
message. Thus a message named only in a reserved entry must not admit the
registered type's generic base.

Everything parses offline: `Framework.cs` declares the bus, the registry and
each handler base as local stand-ins.

| File | What it holds |
| --- | --- |
| `Framework.cs` | `IShoalBus`, `IShoalRegistry` and three handler bases. No base method takes a parameter, so only the carriers decide admission. |
| `Messages.cs` | `ShoalNotice`, `ReefNotice` and `CurrentNotice`. |
| `RegisteredCarriers.cs` | `ShoalIndexer`, `ReefGuard` and `CurrentLogger`, and the installation call that registers all three. |
| `UnregisteredCarriers.cs` | `ReefShoalWatcher`, a carrier of `ShoalBase` that no call registers. |
| `Publishers.cs` | One publisher per message. Each message is sent. |

## Cases

- **Result-only carrier.** `ShoalIndexer : ShoalBase<ShoalNotice>` names
  `ShoalNotice` only in `Dictionary<ShoalNotice, TKey> Index<TKey>()`. The
  result type holds the method's own type parameter, so the extractor writes
  it under `@result:Index`. `ShoalBase` is not admitted, and `ShoalIndexer`
  gets no hop.
- **Base-wide refusal.** `ReefShoalWatcher : ShoalBase<ReefNotice>` takes a
  real `ReefNotice` parameter but is not registered. Its base entered the
  vocabulary only through `ShoalIndexer`, so it gets no hop either.
- **Verbatim method name.** `ReefGuard` receives `ReefNotice` only through
  `public Task @event(ReefNotice notice)`. The key keeps its `@`, but
  `@event` is a method, so `ReefBase` is admitted and `ReefGuard` gets its
  hop.
- **Result member beside a real parameter.** `CurrentLogger` has the same
  result-only member shape as `ShoalIndexer` and also
  `Log(CurrentNotice notice)`. The real parameter admits `CurrentBase`, and
  `CurrentLogger` gets its hop.
