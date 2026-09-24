# bus-signals fixtures

Invented lending-library vocabulary for the native in-repository message-bus
hop: one file per publish idiom, one file per consumer/handler idiom, a
fan-out, and five shapes that must resolve to no hop at all. The bus,
mediator, consumer and handler types are local stand-ins declared in
`Framework.cs` -- the same package-free stance `fixtures/csharp-flowtrace/README.md`
already takes -- so the fixture needs no external reference to parse.

| File | What it exercises |
| --- | --- |
| `Framework.cs` | Local stand-ins every other file calls against: `IBus`, `IMediator`, `IConsumer<T>`, `BaseConsumer<T>`, `IAmInitiatedBy<T>`, `IWorkHandler<T>`, `IRequestHandler<TRequest, TResponse>`, `Batch<T>`, `ISagaInitContext`, plus `Channels` (channel-name constants) and `Telemetry` (a generic-argument logging helper) for the two negative cases that need them. |
| `Messages.cs` | Every message, job, request, reply and response type the shape files below connect through. |
| `GenericPublish.cs` | Positive -- a generic publish, `bus.PublishAsync<LoanRequested>(ct)`. |
| `InlineConstructionPublish.cs` | Positive -- an inline-construction publish, `bus.Publish(new VolumeShelvedMessage { ... }, ct)`, plus its handler `VolumeConsumer`. |
| `IdentifierPublish.cs` | Positive -- an identifier publish, `bus.Publish(msg, ct)`, where `msg` is declared three lines above the call (with an intervening logging call), plus its handler `ReservationConsumer`. |
| `SplitLinePublish.cs` | Positive -- a generic publish whose call chain (`bus` / `.PublishAsync<T>(` / `ct);`) is split across three source lines. |
| `BaseClassConsumer.cs` | Positive -- an inherited consumer: `CatalogueEntryConsumer : BaseConsumer<CatalogueEntryPublishedMessage>`, the target of `SplitLinePublish.cs`. |
| `BatchConsumer.cs` | Positive -- a batch consumer, `LoanBatchConsumer : IConsumer<Batch<LoanRequested>>`. The message is the INNER generic argument (`LoanRequested`), never the wrapper `Batch`; it shares its message with `GenericPublish.cs`/`ConsumerInterfaceImplementation.cs`, since the same publish site should reach both the plain and the batch handler once the inner argument survives extraction. |
| `SagaInitialiserPublish.cs` | Positive -- a saga-initialiser publish, `bus.PublishAsync(context => context.Init(new MembershipLapsedMessage()))`. |
| `SagaInitiator.cs` | Positive -- a saga initiator, `MembershipRenewalSaga : IAmInitiatedBy<MembershipLapsedMessage>`, the target of `SagaInitialiserPublish.cs`. |
| `JobSubmissionPublish.cs` | Positive -- a job-submission verb, `bus.SubmitJob<ShelfAuditJob>(ct)`. |
| `ProcessorShape.cs` | Positive -- a class matching a `*Processor<...>` shape, `CatalogueProcessor : IWorkHandler<ShelfAuditJob>`, the target of `JobSubmissionPublish.cs`. |
| `ReplyPublish.cs` | Positive -- a reply verb, `bus.Reply<LoanApprovalReply>(ct)`, plus its handler `LoanApprovalConsumer`. |
| `MediatorSendPublish.cs` | Positive -- a mediator single-dispatch, `mediator.Send(request)`. |
| `MediatorRequestHandler.cs` | Positive -- a mediator request handler taking the message as its FIRST type argument, `CatalogueLookupHandler : IRequestHandler<CatalogueLookupRequest, CatalogueLookupResult>`, the target of `MediatorSendPublish.cs`. |
| `ConsumerInterfaceImplementation.cs` | Positive -- a plain consumer interface implementation, `ShelfConsumer : IConsumer<LoanRequested>`, sharing `LoanRequested` with `GenericPublish.cs` and `BatchConsumer.cs`. |
| `FanOutHandlers.cs` | Positive -- one message, `OverdueReminderMessage`, with TWO handlers (`OverdueLedgerConsumer`, `OverdueNotifierConsumer`) plus its own publish site (`OverdueReminderTrigger`), pinning fan-out as one edge per handler rather than one edge per message. |
| `UnrelatedPublish.cs` | Negative -- `ArticleDraft.Publish()` is a `Publish` method on a type with no bus, mediator or message relationship at all. Must refuse: the bare verb name `Publish` alone is not enough; there is no `IBus`/`IMediator` receiver here to key off. |
| `DuplicateShortName.cs` | Negative -- `OverdueNotice` is declared twice, once in `BusSignals.Circulation` and once in `BusSignals.Legacy`, neither wired to a publish site or a consumer. Must refuse: a bare short name that resolves to more than one declared type must never be silently picked as an unambiguous message. |
| `DecoyLinePublish.cs` | Negative -- `DecoyAnnouncer.Announce` publishes `ExternalNotice` (unhandled) but names the handled message `LoanRequested` earlier on the SAME source line via `Telemetry.Tag<LoanRequested>()`. Must refuse: a same-line, bare-name match would wrongly key off `LoanRequested` instead of the type actually passed to `Publish`, producing a hop from a site that published something else entirely. |
| `ExternalMessagePublish.cs` | Negative -- `ExternalMessagePublisher.Announce` publishes `System.Uri`, a type declared outside this fixture's own message vocabulary. Must refuse: no consumer in this fixture is registered for it, so the site earns no hop no matter how the verb is read. |
| `ChannelBroadcast.cs` | Negative -- `ShelfUpdateBroadcaster.Broadcast` calls `bus.PublishAsync(Channels.ShelfUpdates, ct)`, naming a channel-name string constant, not a message type. Must refuse: a channel constant is not a type reference at all, so no message-name match is even possible here. |
| `NestedMessageShadowing.cs` | Positive -- `ReturnsDesk` nests its own `LoanRequested`, and both `Trigger` (a publish site) and `Handler` (a consumer), declared inside `ReturnsDesk`, name it by its bare, unqualified name. Both sides must resolve to `ReturnsDesk`'s own nested message, never the top-level one `Messages.cs` declares: `Trigger`'s publish reaches ONLY `Handler`, never `ShelfConsumer`/`LoanBatchConsumer`/`ShelfArchiveConsumer`. A dotted, qualified reference to a nested type is a different ladder path this fixture does not cover. |
| `DuplicateEvidenceConsumer.cs` | Positive -- `ShelfArchiveConsumer` binds `LoanRequested` both on its base list (`IConsumer<LoanRequested>`) and on a qualifying property (`Batch<LoanRequested> Recent`), reached by `GenericPublish.cs`'s single publish site. Must earn exactly one edge for that route, evidence `base-arg`. |
| `SameNamedNonDispatch.cs` | Negative -- `AisleLedger.Publish(LoanRequested entry)` is a same-named, same-arity method this repository itself declares, taking the message's own concrete type. `AisleLedgerWriter.Record`'s call to it must earn no hop despite matching the verb and the message a real route elsewhere in this fixture also uses. |
| `TestDoubleSetupLambda.cs` | Negative -- `bus.Setup(x => x.PublishAsync<LoanRequested>(...))`: Moq's own public verb surface, declared nowhere in this repository, wrapping a lambda that publishes a handled message. Must refuse: the lambda is data a mocking library inspects, never a delegate this engine should read as a dispatch. |
| `TestDoubleVerifyLambda.cs` | Negative -- the verify half of the same shape, `bus.Verify(x => x.PublishAsync<LoanRequested>(...))`. Must refuse for the same reason. |
| `TestDoubleHelperWrapper.cs` | Negative -- `LoanRequestedTestHelpers.ExpectPublish`, a repository-declared helper taking its lambda as `Expression<Action<IBus>>` -- an expression tree, the shape that turns a lambda into data rather than a delegate this engine should read as invoked. Must refuse the `PublishAsync` call inside it, the repository-helper half of the same refusal the two files above cover directly against the library. |
| `PublishingTest.cs` | Positive (`tests` only) -- `ShelfConsumerPublishingTest`, an attribute-carrying (`[Fact]`) test file that PUBLISHES `LoanRequested` -- the same message `ShelfConsumer`/`LoanBatchConsumer` already receive -- over a bus hop. `tests` on either handler must list this file as a possible route, never counted toward the precise test-file/reference counts. |
