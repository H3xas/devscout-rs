# csharp-flowtrace fixture

A tiny, package-free C# solution used to exercise flow-tracer fact
extraction. Every framework-looking type (bus, MVC, hosting) is a local
stand-in, now split across two projects (`Api` and `Shared`), so the
solution restores and builds fully offline.

| File | Fact kind(s) exercised |
| --- | --- |
| `src/Shared/Shared.csproj` | second project in the solution; referenced by `Api` via `ProjectReference` only, so cross-project resolution has a real project boundary to cross |
| `src/Shared/Bus.cs` | Stand-ins: `IConsumer<T>`, `Batch<T>`, `IPublishEndpoint`, `BaseConsumer<T>`, `IMessage`, `ICorrelatedMessage` (moved here from `src/Api/Framework`) |
| `src/Shared/Mvc.cs` | Stand-ins: route/verb attributes, parameter-binding attributes, `ControllerBase`, `IActionResult` results (moved here from `src/Api/Framework`) |
| `src/Shared/Hosting.cs` | Stand-ins: `IServiceCollection` extensions, endpoint routing extensions, `IRequestHandler<TRequest, TResponse>`, and the `Microsoft.AspNetCore.Http.HttpContext` framework-type stand-in (moved here from `src/Api/Framework`) |
| `src/Shared/SharedGroups.cs` | a route group (`Admin`) declared in the referenced project rather than the entry-point project, for cross-project route-group resolution |
| `src/Api/Messaging/Messages/ParcelDispatchedMessage.cs` | message_class by path+suffix (`ParcelDispatchedMessage`), message_class by marker interface only (`DeliveryScheduled`), and a message that qualifies by the path rule alone (`RouteNote`) |
| `src/Api/Messaging/Events/ParcelLostEvent.cs` | message_class by marker interface outside the Messages path (`ParcelLostEvent`), plus a plain type that is not a message (`DispatchAudit`) |
| `src/Api/Messaging/Events/ReturnRequested.cs` | message_class by marker interface, feeding the partial-class consumer below |
| `src/Api/Consumers/ParcelDispatchedConsumer.cs` | consume fact via a primary-constructor consumer; publish of a local variable (`bus.Publish(note, ct)`); publish via `SubmitJob` |
| `src/Api/Consumers/DeliveryScheduledConsumer.cs` | consume fact via a base-class consumer (`BaseConsumer<T>`); ctor-assigned fields with a null-guard; publish with an explicit type argument inside a private helper method |
| `src/Api/Consumers/BatchConsumer.cs` | consume fact via `Batch<T>` unwrapping (`ParcelBatchConsumer`); an abstract consumer that yields no consume fact (`AuditingConsumer<T>`) |
| `src/Api/Consumers/ReturnRequestedConsumer.cs`, `ReturnRequestedConsumer.Audit.cs` | consume fact for a `partial class` split across two files: one consume fact, attributed to the part carrying the base list, not duplicated on the part that only holds a private helper |
| `src/Api/Controllers/ParcelsController.cs` | route facts: class-level route + bare verb, `Route` combined with two bare verbs, `[action]` token with no verb, and a private helper that yields only a method_span; an inline message publish |
| `src/Api/Handlers/GetParcelHandler.cs` | di_binding via `IRequestHandler<TRequest, TResponse>`; a request/response record pair |
| `src/Api/Repositories/IParcelRepository.cs`, `ParcelRepository.cs` | iface_impl for two independent interface/implementation pairs |
| `src/Api/Billing/Invoice.cs` | a namespace-prefix trap: `Systematic.Billing` shares a letter-prefix with a framework root without being under its namespace tree |
| `src/Api/Program.cs` | minimal-API route facts from top-level statements: primitive vs fixture-type lambda parameters, framework/special-type exclusion (`HttpContext`, `CancellationToken`), an inline group chain feeding a method-group handler, a two-verb registration, a publish of a lambda-local message, a route bound through a cross-project route group (`SharedGroups.Admin`), and a route whose parameter type lives under the namespace-prefix-trap namespace (`Invoice`) |
| `src/Api/Endpoints/ParcelEndpoints.cs` | method_span for static handler methods referenced as method groups; a route fact registered from inside a named method (`Register`), including a route reached through the cyclic route groups below |
| `src/Api/Endpoints/LegacyGroups.cs` | two static properties that reference each other on purpose, so a route walk following group-builder references needs a revisit guard; the class is never executed |
| `src/Api/Messaging/InMemoryBus.cs` | the concrete `IPublishEndpoint` implementation used by DI registration |
| `src/Api/Contracts/CreateParcel.cs` | a plain fixture-type request contract used for ctor_field resolution |
