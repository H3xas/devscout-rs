# csharp-semantic fixtures

Each case below is a scenario the semantic resolver's test suite exercises.
Cases are identified by the same letter/tag used in the corresponding test
names; several cases span more than one file.

| Case | File(s) | What it exercises |
| --- | --- | --- |
| a | src/App/AppDbContext.cs, src/Domain/Configs.cs | `FilterConfig.Property` / `ColumnConfig.Property` are POCO properties that share a name with EF's `Entity<T>.Property(...)` fluent method; checks that the name collision does not produce a false positive while the precise `HasKey`/`Name`/`Total` hits still resolve. |
| b | src/App/Worker.cs, src/Ext.Adapters/UpgradeLogAdapter.cs, src/Ext.Contracts/IUpgradeLog.cs | `ILogger<Worker>` extension calls (`LogInformation`/`LogWarning`) are external framework calls and must never resolve into the in-tree `IUpgradeLog`/`UpgradeLogAdapter` members of the same name, contrasted with a field-typed `IUpgradeLog` call that does resolve precisely. |
| c | src/Ext.Adapters/ServiceCollectionExtensions.cs | `AddWidgets` is a static extension method on `IServiceCollection`; exercises extension-method resolution. |
| c1 | src/App/Worker.cs | A local `using` for a namespace, redundant with a project-wide `global using` elsewhere, still resolves the same static class. |
| c2 | src/Ext.Adapters/ServiceCollectionExtensions.cs | A call resolves with no `using` directive at all, because the caller is lexically nested inside the same namespace (an enclosing namespace rather than an explicit import). |
| c3 | src/App/AppDbContext.cs | A file-scoped `global using` is consumed project-wide, contrasted with the local `using` (c1) and the enclosing-namespace case (c2). |
| d | src/App/ApiClient.cs, tests/App.Tests/FakeServer.cs, tests/App.Tests/WorkerTests.cs | `HttpClient.GetAsync` shares a name with a test-only `FakeServer.GetAsync` used only from the test project; the resolver must not guess across projects that cannot structurally reach each other. |
| e | src/App/ApiClient.cs, src/Unreachable/Mailer.cs | `Queue<string>.Enqueue` shares a name with `Mailer.Enqueue` in a project nothing references; the resolver must not guess into a structurally unreachable project. |
| enum | src/Domain/Order.cs, src/App/Worker.cs | `OrderStatus` backs the two-spelling enum-member resolution (`Ns.E.Member` vs `Ns.E`), exercised by an `order.Status == OrderStatus.Open` comparison. |
| f | src/Domain/Order.cs, src/Domain/Order.Validation.cs | `Order` is a partial class split across two files; member bookkeeping must attribute members from both files to the same definition rather than double-counting or splitting them. |
| g-chain | src/App/ApiClient.cs, tests/App.Tests/WorkerTests.cs | A nested member-access chain (for example `response.StatusCode.ToString()`, or a static-qualified call-chain tail) exercises resolution of a chained qualifier. |
| g-cond | src/App/ApiClient.cs | A conditional-access expression (`_http?.Dispose()`) resolves to the same target a plain access would. |
| g-await | src/App/Worker.cs | An awaited call on a field receiver gets a call fact owned by the field's type, unlike an awaited call on a bare type/static qualifier, which gets none. |
| g-this | src/Domain/Order.Validation.cs | A `this.Name` qualifier resolves through the enclosing type's own declaration across a partial-class file boundary. |
| h | src/Domain/AuditableEntity.cs, src/Domain/Shipment.cs | A protected member and a public member declared on a base class are both called from a derived class through `base.`. |
| i | src/Domain/Order.cs, src/Domain/Order.Validation.cs | A field declared in one file of a partial class is used bare (unqualified) from a sibling file of the same partial class; exercises the cross-file field-typing table. |
| j | src/Domain/AuditableEntity.cs, src/Domain/Shipment.cs | A protected field declared on a base class is used bare from a derived class; exercises the same cross-file field-typing table as case i, but walked across inheritance instead of a partial-class sibling. |
| k | src/Domain/Order.cs, src/App/Worker.cs | A local variable is typed through a cast, an `is` pattern designation, and an explicit `out` argument; a member is then called on each typed local. |
| l | src/App/Repo.cs, src/App/Worker.cs | A static-qualified async method returns `Task<Order>`; the caller awaits it and uses the result, exercising the one-layer `Task<T>` unwrap for an awaited static-qualifier local. |
| m | src/App/ApiClient.cs, src/App/Worker.cs | A method returns `Order` directly (no `await`) and is called as one expression with a chained `.Validate()`, exercising the one-hop call-chain tail. |
| n | src/App/Worker.cs | A lambda parameter typed from a single-type-argument generic (`List<Order>`) earns a type fact; the same lambda pattern over a two-type-argument generic (`Dictionary<string, Order>`) does not, and falls to the scored tier. |
| o | src/Domain/Order.cs, src/Domain/OrderChannel.cs, src/App/Worker.cs | A two-parameter instance method and a same-named one-parameter extension method coexist; the call's argument count decides which one binds. |
| p | src/Domain/IWidget.cs, src/Domain/Widget.cs, src/App/Worker.cs | An interface and the class that implements it both declare a same-named method; a call on a class-typed receiver must always bind the class's own declaration, never the interface's. |
