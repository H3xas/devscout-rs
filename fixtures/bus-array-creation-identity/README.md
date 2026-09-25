# bus-array-creation-identity fixtures

A coastal-watch estate, invented and neutral: nothing here reuses
`fixtures/bus-signals/`'s, `fixtures/bus-vocabulary/`'s,
`fixtures/bus-vocabulary-admission/`'s, `fixtures/bus-extension-refusal/`'s or
`fixtures/bus-optional-parameter-alignment/`'s own vocabulary, and no element type here
is declared in `fixtures/bus-array-identity/` -- a new element type added to that
directory could collide with its own suffix-matching tests, so these two cases live
in their own directory with their own types. `fixtures/bus-array-identity/ArraySpellings.cs`
shows array creations REACHING an array consumer; each case here shows an array-creation
spelling NEVER reaching the single-message consumer of its element type. Together with
`fixtures/bus-array-identity/ArrayNeverReachesSingle.cs` (a declared array-typed local), they
check that direction for three array spellings.

Everything parses offline: `Framework.cs` declares the bus and the consumer interface as
local stand-ins, so no package reference is needed.

| File | What it covers |
| --- | --- |
| `Framework.cs` | The local stand-ins: `IWatchBus` (`Publish<T>`) and `IConsumer<TMessage>`. |
| `Messages.cs` | `BeaconSighting` and `TideGauge`, the two notices the cases below connect through. |
| `ExplicitInitializerArray.cs` | `BeaconOfficer : IConsumer<BeaconSighting>` and `BeaconBulletin : IConsumer<BeaconSighting[]>` both exist; an EXPLICIT array creation WITH an initializer (`new BeaconSighting[] { new BeaconSighting() }`) reaches only `BeaconBulletin`, with the `[]` suffix on its edge, and a plain single publish of `BeaconSighting` reaches only `BeaconOfficer`. |
| `ImplicitlyTypedArray.cs` | `TideOfficer : IConsumer<TideGauge>` and `TideGaugeBulletin : IConsumer<TideGauge[]>` both exist; an IMPLICITLY TYPED array creation (`new[] { new TideGauge(), new TideGauge() }`) reaches only `TideGaugeBulletin`, with the `[]` suffix on its edge, and a plain single publish of `TideGauge` reaches only `TideOfficer`. |

## Cases

- **An explicit array creation with an initializer never reaches a single message
  consumer.** `ExplicitInitializerArray.cs`:
  `an_explicit_array_creation_with_an_initializer_never_reaches_a_single_message_consumer`.
- **An implicitly typed array creation never reaches a single message consumer.**
  `ImplicitlyTypedArray.cs`:
  `an_implicitly_typed_array_creation_never_reaches_a_single_message_consumer`.
