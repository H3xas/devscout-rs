# bus-vocabulary fixtures

A lending-library estate that runs its own message bus. Nothing here is named
after a bus the engine ships handling for: the base is `ShelfWorkerBase<T>`,
the registry is `IWorkshopRegistry`, the binding is `Chime<T>`. A hop found in
this fixture was found because the repository's own registration calls said
where to look, which is the whole point of the directory.

Everything parses offline: `Framework.cs` declares the bus, the registry and
the handler bases as local stand-ins, so no package reference is needed.

| File | What it covers |
| --- | --- |
| `Framework.cs` | The local stand-ins: `IShelfBus` (with both a recognized `Publish` and a house-only `Enqueue`), `ShelfWorkerBase<T>`, `IBindingHandler<T>`, `Chime<T>`, and `IWorkshopRegistry` with its two one-type-argument installation methods. |
| `Messages.cs` | The notices the cases below connect through. |
| `RegisteredWorker.cs` | Positive — `AddShelfWorker<ReturnsDeskWorker>()` puts `ShelfWorkerBase` into the vocabulary, and `ReturnsDeskWorker` becomes reachable from the publisher of its base's own type argument. |
| `InheritedWorker.cs` | Positive — `BranchWorkerBase<T>` passes its type parameter straight through to `ShelfWorkerBase<T>`, so the intermediate names no message and its subclass `OverdueWorker` is where the message is. |
| `BoundMessages.cs` | Positive — `ShelfAuditHandler` is registered through `IBindingHandler<ReturnsDeskNotice>` and binds a second notice on a `Chime<ShelfAuditNotice>` property its base list never names. Also the fan-out case: it shares `ReturnsDeskNotice` with `ReturnsDeskWorker`. |
| `ForwardingWrapper.cs` | Positive and negative in one file — `DispatchDesk.Forward<T>` hands its caller's notice to `Publish`, so calls to `Forward` are publish sites; `SecondHandDesk.Relay<T>` wraps `Forward` in turn and is NOT promoted, because forwarding is followed one hop only. |
| `Unregistered.cs` | Negative — `LedgerReaderBase<T>` is as message-shaped as any base here, and nothing registers a type deriving it, so it stays outside the vocabulary and earns no hop. |
| `HouseVerbOnly.cs` | Negative — `AisleWorker` is registered and its base is in the vocabulary, but its publisher calls `Enqueue`, a verb no recognized publish is written beside. The handler side is learned; the publish side is not, and the hop is missed. |

`HouseVerbOnly.cs` is the fixture's own record of a known limit: a
repository's handler vocabulary is derived from its registrations, and its
publish vocabulary is not derived at all. A house bus whose verb resembles no
recognized one is invisible on the publishing side even when its handlers are
read correctly.
