# transport-masstransit fixture

A small repository of invented vocabulary that states MassTransit transport
configuration in every shape the transport facts read, and in the shapes they
refuse. Nothing here is built, and no package is referenced. The project files
give the project model, the SDK's implicit usings and the `Using` items
something to read. `tests/transport_facts.rs` maps a copy and reads
the facts `graph.json` and `devscout export` record.

| Case | File(s) | What it covers |
| --- | --- | --- |
| file using | `src/Service/Program.cs` | A `using MassTransit;` directive qualifies the bus root and every configurator below it. |
| implicit usings | `src/Service/Service.csproj`, `src/Service/Program.cs` | The web SDK's implicit usings import the options-binding extension methods that no `using` names. |
| using item | `src/Worker/Worker.csproj`, `src/Worker/WorkerBus.cs` | `<Using Include>` items import `MassTransit`, `System` and the dependency-injection namespace. An aliased item adds nothing. |
| global using file | `src/Billing/GlobalUsings.cs`, `src/Billing/BillingBus.cs`, `src/Worker/GlobalUsings.cs` | A `global using` file imports the namespaces for every file of its project. |
| nested lambdas | `src/Service/Program.cs` | `AddMassTransit`, then `UsingRabbitMq`, then `ReceiveEndpoint`, each lambda parameter typed by the catalogue entry that holds it. |
| dotted chain start | `src/Service/Program.cs` | `builder.Services.AddMassTransit(…)` starts a chain on an untyped receiver. |
| static entry points | `src/Service/Program.cs`, `src/Billing/BillingBus.cs` | `EndpointConvention.Map` and `Bus.Factory.CreateUsingRabbitMq`. |
| out-of-scope lookalike | `src/Worker/WorkerBus.cs`, `src/Refusals/Fakes.cs` | A chain start on `new ServiceCollection()` keeps its fact, because the repository's own `ServiceCollection` is in a namespace the file does not import. |
| declared receivers | `src/Billing/BillingBus.cs`, `src/Billing/Values.cs`, `src/Service/Consumers/Consumers.cs` | A field, a property, a constructor-assigned field, a typed local, a method parameter and a `ConsumeContext<T>` parameter. |
| registrations | `src/Service/Program.cs` | A consumer, a consumer with its definition, a state machine, a namespace scan and three request clients. |
| receive endpoint types | `src/Service/Program.cs` | A configured consumer, a handler and a configured saga, each with its own line. |
| endpoint names | `src/Service/Consumers/Consumers.cs` | `EndpointName` and `Endpoint(e => e.Name = …)` in a consumer definition. |
| entity names | `src/Service/Program.cs`, `src/Worker/WorkerBus.cs`, `src/Billing/Values.cs` | A constant concatenation, an AWS resource name and a `nameof`. |
| formatters | `src/Service/Program.cs`, `src/Billing/BillingBus.cs`, `src/Worker/WorkerBus.cs` | A bus formatter, a formatter argument with its constructor arguments, a type outside the catalogue, a filter and the default. |
| outbox | `src/Service/Program.cs`, `src/Service/Consumers/Consumers.cs` | The outbox registration and the bus outbox inside it. |
| options bindings | `src/Service/Program.cs`, `src/Billing/BillingBus.cs`, `src/Worker/WorkerBus.cs`, `src/Refusals/Unbound.cs` | `BindConfiguration` and `Configure<T>(GetSection(…))`, with values from two `appsettings` files. One `appsettings` file does not parse, and no file states one key. |
| literal URIs | `src/Service/Program.cs`, `src/Billing/BillingBus.cs`, `src/Worker/WorkerBus.cs` | The `queue:`, `exchange:`, `topic:`, `rabbitmq://` and `amazonsqs://` forms. |
| constants | `src/Contracts/QueueNames.cs`, `src/Service/Program.cs`, `src/Service/Consumers/Consumers.cs` | A constant in another file, a concatenation and a constant of a constant. |
| unknown values | `src/Billing/Values.cs`, `src/Billing/BillingBus.cs`, `src/Contracts/QueueNames.cs` | One value for each reason word. The source forms are an interpolated string, a parameter, a local, a `static readonly` field, an instance field, a method call, `nameof` and an expression. The other forms are a package constant, an unresolved constant, a cycle, an unknown scheme, a malformed URI, an unreadable value and disagreeing overloads. |
| scopes | `src/Service/Program.cs`, `src/Billing/BillingBus.cs`, `src/Worker/WorkerBus.cs` | One queue name on three buses with different scopes. A URI host over the bus host, two host calls, and a bus with no host call. |
| secrets | `src/Service/appsettings.json`, `src/Service/appsettings.Development.json`, `src/Billing/appsettings.json`, `src/Billing/BillingBus.cs` | Passwords in an unmarked key, in a connection string, in URI user information and in a marked key whose value fails its grammar. |
| test substitution | `tests/Fixture.Tests/OrderFlowTests.cs`, `src/Service/LocalHarness.cs` | `AddMassTransitTestHarness` inside a `WebApplicationFactory` host in a test project, and the same call in a production file. |
| lookalike type | `src/Refusals/Fakes.cs`, `src/Refusals/LookalikeType.cs` | A repository type with a catalogue type's simple name, in scope beside `MassTransit`. |
| missing using | `src/Refusals/MissingUsing.cs` | A catalogue receiver type in a file that imports no catalogue namespace. |
| shadowed chain start | `src/Refusals/Shadowing.cs` | A repository extension method named like the bus root, in a namespace the call imports. |
| incompatible receivers | `src/Refusals/Incompatible.cs` | A catalogue member on a catalogue type that does not declare it. A chain start on a repository type. A catalogue member name on a repository type. |
| undefended receivers | `src/Refusals/Undefended.cs` | A `using` alias, a `var` from a call and a `using static` entry point, which no fact reads. |
| shared contracts | `src/Contracts/Contracts.csproj`, `src/Contracts/Messages.cs` | The message types the other projects reference. |
