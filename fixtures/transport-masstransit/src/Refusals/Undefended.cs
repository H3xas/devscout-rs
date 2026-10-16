using System;
using MassTransit;
using Cfg = MassTransit.IRabbitMqBusFactoryConfigurator;
using static MassTransit.EndpointConvention;
using Fixture.Contracts;

namespace Fixture.Refusals
{
    public class Undefended
    {
        public void Alias(Cfg cfg)
        {
            cfg.ReceiveEndpoint("refused-alias", e => { });
        }

        public void Untyped()
        {
            var cfg = Locate();
            cfg.ReceiveEndpoint("refused-var", e => { });
            Map<SubmitOrder>(new Uri("queue:refused-static"));
        }

        private static dynamic Locate() => null;
    }
}
