using System;
using MassTransit;
using Microsoft.Extensions.DependencyInjection;
using Fixture.Refusals.Fakes;

namespace Fixture.Refusals
{
    public class Incompatible
    {
        public void Configure(IReceiveEndpointConfigurator endpoint, RouterBuilder builder, OrderRouter router)
        {
            endpoint.Host("refused-incompatible-host");
            builder.AddMassTransit(x => x.UsingRabbitMq((context, cfg) => cfg.ReceiveEndpoint("refused-repo-receiver", e => { })));
            router.ReceiveEndpoint("refused-repo-member", e => { });
        }
    }
}
