# bus-vocabulary-admission fixtures

A harbor-pilotage estate, invented and neutral: nothing here is named after a
bus the engine ships handling for, or after `fixtures/bus-vocabulary/`'s own
lending-library vocabulary, which this directory leaves untouched.

Every case below probes the same question: a registered type's own generic
base is not proof by itself that the base is a handler shape. The base only
earns a place in the vocabulary when the registered type is not itself a
sent message AND actually receives, through a parameter or a bound property,
a message the corpus does send. `System.IEquatable<T>` is the only library
type this fixture names, standing in for the shape that let a library
equality base contaminate a real corpus's vocabulary.

Everything parses offline: `Framework.cs` declares the bus, the registry and
every handler base as local stand-ins, so no package reference is needed.

| File | What it covers |
| --- | --- |
| `Framework.cs` | The local stand-ins: `IPilotBus`, `IPilotRegistry` and six handler bases, each shaped to isolate one admission clause. |
| `Messages.cs` | The messages the cases below connect through, including `PilotRequest` (equatable to itself) and `SelfCarriedNotice` (itself registered as a handler). |
| `RegisteredCarriers.cs` | Every registered carrier: two positive (`PilotOfficer`, `BerthGuard`), the bulk sweep that registers `PilotRequest` itself, and four whose registration fails admission for a different stated reason each (`TerminalWatcher`, `SilentWatcher`, `EchoWatcher`, `SelfCarriedNotice`), plus the wrapped-parameter positive (`BerthBatchHandler`). |
| `UnregisteredCarriers.cs` | Three classes named in no registration call, each carrying a base whose only registered carrier failed for a different reason, each otherwise a perfect candidate against `BerthNotice` (sent, properly received everywhere else). A spurious hop on any of these means the base wrongly entered the vocabulary through its failed registration. |
| `Publishers.cs` | One publisher per sent message. `TerminalNotice` is the one message this fixture never sends. |

## Cases

- **Self-route refused.** `PilotRequest : IEquatable<PilotRequest>` is sent
  and is swept into the registry by the bulk installation call alongside the
  real handlers. `IEquatable` earns no place in the vocabulary: `PilotRequest`
  is itself the message its own argument names, so the corpus never "sends a
  message `PilotRequest` receives" without also failing "`PilotRequest` is
  not itself sent."
- **Registered-message base refused entirely.** `SelfCarriedNotice` carries
  `LedgerBase<BerthNotice>` and is itself sent, so `LedgerBase` is refused
  through this registration regardless of `BerthNotice`'s own standing.
  `LedgerReader`, an unregistered carrier of the same base against the same
  message, proves the refusal is base-wide: it gets no hop either, though
  `BerthNotice` is sent and properly received.
- **Unsent argument refused.** `TerminalWatcher`'s own argument
  (`TerminalNotice`) is never sent, so `TerminalBase` earns no place in the
  vocabulary. `SecondTerminalWatcher`, an unregistered carrier of the same
  base against `BerthNotice`, gets no hop either.
- **Never-received argument refused.** `SilentBase`'s own method returns its
  argument but never takes it as a parameter; `SilentWatcher`'s argument
  (`TideNotice`) is sent, but nothing ever receives it. `SecondSilentWatcher`,
  an unregistered carrier against `BerthNotice`, gets no hop either.
- **Binding-only.** `ChimeWatcherBase`'s own method takes no parameter, so
  `EchoWatcher`'s base argument (`TideNotice`, sent) can never admit it. Its
  bound property (`Chime<EchoNotice>`, sent) does admit `ChimeWatcherBase`,
  binding-only: `EchoWatcher` reaches its publisher for `EchoNotice` through
  the property, never for `TideNotice` through its base argument.
- **Wrapped receiving.** `BerthBatchHandler`'s own method takes
  `List<BerthNotice>`, not bare `BerthNotice`; the receiving check has to see
  through the wrapper to admit `WrappedHandlerBase`.
