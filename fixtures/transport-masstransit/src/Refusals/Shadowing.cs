using System;
using MassTransit;
using Microsoft.Extensions.DependencyInjection;
using Fixture.Refusals.Shadow;

namespace Fixture.Refusals.Shadow
{
    public static class LocalBus
    {
        public static IServiceCollection AddMassTransit(this IServiceCollection services, Action<object> configure)
        {
            return services;
        }
    }
}

namespace Fixture.Refusals
{
    public class Shadowing
    {
        public void Configure(IServiceCollection services)
        {
            services.AddMassTransit(x => x.UsingRabbitMq((context, cfg) => cfg.ReceiveEndpoint("refused-shadowed", e => { })));
        }
    }
}
