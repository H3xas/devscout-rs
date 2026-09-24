# bus-array-identity fixtures

A coastal-watch estate, invented and neutral: nothing here reuses
`fixtures/bus-signals/`'s lending-library vocabulary or
`fixtures/bus-vocabulary/`'s or `fixtures/bus-vocabulary-admission/`'s own
names. Every case below probes one question: a message and an array of that
same message are different identities on both sides of a hop, so a single
publish never reaches an array consumer and an array publish never reaches a
single-message consumer.

Everything parses offline: `Framework.cs` declares the bus and the consumer
interface as local stand-ins, so no package reference is needed.

| File | What it covers |
| --- | --- |
| `Framework.cs` | The local stand-ins: `IWatchBus` (`Publish<T>`, `Send`) and `IConsumer<TMessage>`. |
| `Messages.cs` | The notices the cases below connect through. |
| `SingleNeverReachesArray.cs` | `AlertOfficer : IConsumer<AlertNotice>` and `AlertBulletin : IConsumer<AlertNotice[]>` both exist; a single generic-argument publish of `AlertNotice` reaches only `AlertOfficer`. |
| `ArrayNeverReachesSingle.cs` | `StormOfficer : IConsumer<StormWarning>` and `StormBulletin : IConsumer<StormWarning[]>` both exist; a `StormWarning[]`-declared local published by the identifier spelling reaches only `StormBulletin`. This is one of the spellings (`fact.is_array`) that silently lost its array bit before extraction carried it through -- `fixtures/bus-array-creation-identity/` covers the other two, an explicit array creation with an initializer and an implicitly typed array creation, in the same never-reaches-a-single-consumer direction. |
| `ArraySpellings.cs` | `TideBulletin : IConsumer<TideNotice[]>` reached by all four array-publish spellings: the generic argument (`Publish<TideNotice[]>`), an explicit array creation (`new TideNotice[2]`), an implicit array creation (`new[] { new TideNotice(), new TideNotice() }`), and a declared array-typed local (`Send`). Every edge's own `message` carries the `[]` suffix. |
| `MixedImplicitArray.cs` | An implicit array creation whose two elements construct different types (`new[] { new TideNotice(), new HarborNotice() }`) names no message and earns no hop at all. |
| `PropertyBoundArray.cs` | `HarborWarden : IConsumer<HarborNotice>` also binds `List<HarborNotice[]>` on a property. A single publish of `HarborNotice` reaches it through the base argument (`base-arg`); a `HarborNotice[]` publish reaches the SAME def only through the property (`property-arg`) -- the array is bound apart from its element, never through the base. |
| `TypeParameterArrayGuard.cs` | `Envelope<TMessage> : IConsumer<TMessage[]>` is an open generic whose own array argument is the class's own type parameter (recorded as the wildcard `*`, never a concrete array): it earns no message and makes no wrapper, so it gains no hop from anything. `ReliefOfficer`/`ReliefDispatcher` is the ordinary positive control proving the rest of the fixture still routes normally. |

## Cases

- **Single never reaches array.** `SingleNeverReachesArray.cs`:
  `a_single_message_publish_never_reaches_an_array_consumer`.
- **Array never reaches single, via the spelling that used to lose its own
  suffix.** `ArrayNeverReachesSingle.cs`:
  `an_array_publish_never_reaches_a_single_message_consumer`.
- **Every array-publish spelling reaches the array consumer, suffix
  included.** `ArraySpellings.cs`:
  `an_array_publish_reaches_the_array_consumer_of_its_element_type`.
- **A mixed implicit array names no message.** `MixedImplicitArray.cs`:
  `an_implicit_array_of_mixed_constructions_names_no_message`.
- **A property-bound array is a separate identity from its own element.**
  `PropertyBoundArray.cs`:
  `a_property_bound_array_message_is_bound_apart_from_its_element`.
- **An open generic's own type-parameter array is not a message.**
  `TypeParameterArrayGuard.cs` (guard, passes unmodified):
  `a_type_parameter_array_names_no_message_and_makes_no_wrapper`.
