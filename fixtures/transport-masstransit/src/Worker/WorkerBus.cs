namespace Fixture.Worker;

public static class WorkerBus
{
    public static void Configure(IServiceCollection services, IConfiguration configuration)
    {
        services.AddMassTransit(x =>
        {
            x.UsingAmazonSqs((context, cfg) =>
            {
                cfg.Host("us-east-2", h => { });
                cfg.ReceiveEndpoint("order-intake", e => { });
                cfg.Message<OrderSubmitted>(m => m.SetEntityName("arn:aws:sns:us-east-2:123456789012:order-submitted"));
                cfg.ConfigureEndpoints(context);
            });
        });

        services.Configure<AmazonSqsTransportOptions>(configuration.GetSection("Sqs"));
    }

    public static async Task Forward(ISendEndpointProvider provider)
    {
        await provider.GetSendEndpoint(new Uri("amazonsqs://us-east-2/ledger/order-archive"));
    }
}
