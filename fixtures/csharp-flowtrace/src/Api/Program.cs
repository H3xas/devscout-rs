// Top-level statements exercising minimal-API route registration: a bound
// lambda with a primitive and a fixture-type parameter, a lambda that binds
// a framework/special type meant to be excluded from ctor_field, an inline
// group chain feeding a method-group handler, a two-verb registration, and
// a publish of a lambda-local message.
using Courier.Api.Contracts;
using Courier.Api.Endpoints;
using Courier.Api.Messaging;
using Courier.Api.Messaging.Messages;
using Courier.Api.Repositories;
using Courier.Framework;
using Microsoft.AspNetCore.Http;
using Systematic.Billing;

var app = WebApplication.Create();
app.Services.AddScoped<IParcelRepository, ParcelRepository>();
app.Services.AddSingleton<ILabelPrinter>(sp => new ZplLabelPrinter());
app.Services.AddTransient<IPublishEndpoint, InMemoryBus>();

var parcels = app.MapGroup("api/parcels");
parcels.MapGet("{id}", (int id, IParcelRepository repository) => repository.Find(id));
parcels.MapPost("", async (CreateParcel request, IPublishEndpoint bus, HttpContext http, CancellationToken ct) =>
{
    var message = new ParcelDispatchedMessage(request.Recipient, request.Address);
    await bus.Publish(message, ct);
});
app.MapGroup("api").MapGroup("labels").MapDelete("{id}", ParcelEndpoints.Delete);
app.MapMethods("health", new[] { "GET", "HEAD" }, () => "ok");
parcels.MapPut("{id}", ParcelEndpoints.Update);
parcels.MapPatch("{id}/invoice", (int id, Invoice invoice, IParcelRepository repository) => repository.Find(id));

SharedGroups.Admin.MapGet("stats", () => "ok");

app.Run();
