# bus-non-public-overload fixtures

A relay estate, invented and neutral: it reuses no vocabulary from `fixtures/bus-signals/`,
`fixtures/bus-vocabulary/`, `fixtures/bus-vocabulary-admission/`, `fixtures/bus-array-identity/`,
`fixtures/bus-array-creation-identity/`, `fixtures/bus-extension-refusal/` or
`fixtures/bus-optional-parameter-alignment/`. A mocking library's setup call is recognized by its
verb when the repository declares no callable overload of that verb. A private method that happens
to share the verb's name is not such an overload: it must not turn the library's setup lambda into
a plain delegate call that keeps its hop.

Everything parses offline: `Framework.cs` declares the bus and the consumer interface as local
stand-ins, and the mocking library's `Mock<T>` is referenced by its `using` alone, as a package
type the repository does not declare.

| File | What it covers |
| --- | --- |
| `Framework.cs` | The local stand-ins: `IRelayBus` (`Publish<T>`), `IConsumer<TMessage>`, and `Match.Any<T>()`, a matcher stand-in. |
| `Messages.cs` | `Flare`, the one message this fixture sends and handles. |
| `FlareWatch.cs` | `FlareWatcher : IConsumer<Flare>` and `FlareLauncher`, a real publish of `Flare`: the fixture's ordinary hop. |
| `RelayStation.cs` | An unrelated class declaring `private void Setup(Action<IRelayBus> configure)`, a plain delegate parameter. |
| `LibrarySetupLambda.cs` | `relay.Setup(x => x.Publish<Flare>(Match.Any<Flare>(), default))` on a `Mock<IRelayBus>`: the library's setup lambda, which publishes nothing at run time. This line earns no hop. |

## Cases

- **A mocking library's setup lambda emits no hop when an unrelated class declares a private
  `Setup` taking a plain delegate.** `LibrarySetupLambda.cs` beside `RelayStation.cs`:
  `a_mocking_setup_lambda_emits_no_hop_when_an_unrelated_class_declares_a_private_setup_taking_a_plain_delegate`.
  The same test checks that `FlareWatch.cs`'s real publish keeps its hop.
