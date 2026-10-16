using MassTransit;

namespace Shop.Transport
{
    public static class ShopBus
    {
        public const string Intake = "order-intake";

        public static void Configure(IServiceCollection services)
        {
            services.AddMassTransit(x =>
            {
                x.AddConsumer<IntakeConsumer>();
                x.UsingRabbitMq((context, cfg) =>
                {
                    cfg.Host("broker.example", "orders");
                    cfg.ReceiveEndpoint(Intake, e => e.ConfigureConsumer<IntakeConsumer>(context));
                    cfg.ConfigureEndpoints(context, new KebabCaseEndpointNameFormatter("shop", false));
                });
            });
            services.AddOptions<RabbitMqTransportOptions>().BindConfiguration("Broker");
        }
    }

    public class IntakeConsumer : IConsumer<OrderPlaced>
    {
        public async Task Consume(ConsumeContext<OrderPlaced> context)
        {
            await context.GetSendEndpoint(new Uri("queue:order-archive"));
        }
    }

    public record OrderPlaced(string Id);
}
