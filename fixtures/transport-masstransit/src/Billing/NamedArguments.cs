namespace Fixture.Billing
{
    public static class NamedArguments
    {
        public static void Reordered(IServiceCollection services)
        {
            services.AddMassTransit(x => x.UsingRabbitMq((context, cfg) =>
            {
                cfg.Host(virtualHost: "named-vhost", host: "named-broker.internal");
                cfg.ReceiveEndpoint("named-intake", e => { });
            }));
        }

        public static void InPlace(IServiceCollection services)
        {
            services.AddMassTransit(x => x.UsingRabbitMq((context, cfg) =>
            {
                cfg.Host("ordered-broker.internal", virtualHost: "ordered-vhost");
                cfg.ReceiveEndpoint("ordered-intake", e => { });
                cfg.ConfigureEndpoints(context, new KebabCaseEndpointNameFormatter("ordered", includeNamespace: true));
            }));
        }
    }
}
