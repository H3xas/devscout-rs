using MassTransit;
using Fixture.Contracts;
using Fixture.Service.Consumers;

var builder = WebApplication.CreateBuilder(args);

builder.Services.AddMassTransit(x =>
{
    x.SetKebabCaseEndpointNameFormatter();
    x.AddConsumer<SubmitOrderConsumer>();
    x.AddConsumer<AuditConsumer, AuditConsumerDefinition>();
    x.AddSagaStateMachine<OrderStateMachine, OrderState>();
    x.AddConsumersFromNamespaceContaining<SubmitOrderConsumer>();
    x.AddRequestClient<CheckStock>(new Uri("exchange:check-stock"));
    x.AddRequestClient<CheckCredit>();
    x.AddRequestClient(typeof(InvoiceDue), new Uri("rabbitmq://credit.internal/credit/check-credit"));
    x.AddEntityFrameworkOutbox<OrdersDbContext>(o => o.UseBusOutbox());

    x.UsingRabbitMq((context, cfg) =>
    {
        cfg.Host("broker.internal", "ledger", h => { });

        cfg.ReceiveEndpoint("order-intake", e =>
        {
            e.ConfigureConsumer<SubmitOrderConsumer>(context);
            e.Handler<OrderNoted>(note => Task.CompletedTask);
        });

        cfg.ReceiveEndpoint(QueueNames.Audit, e => e.ConfigureSaga<OrderState>(context));

        cfg.Message<OrderSubmitted>(m => m.SetEntityName(QueueNames.Prefix + "order-submitted"));

        cfg.ConfigureEndpoints(context);
    });
});

builder.Services.AddOptions<RabbitMqTransportOptions>().BindConfiguration("Broker");

EndpointConvention.Map<SubmitOrder>(new Uri("queue:submit-order"));
EndpointConvention.Map<ShipmentBooked>(new Uri("topic:shipment-events"));

var app = builder.Build();
app.Run();

public partial class Program
{
}
