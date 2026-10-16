using MassTransit;
using Microsoft.Extensions.DependencyInjection;

namespace Fixture.Refusals
{
    public static class Unbound
    {
        public static void Bind(IServiceCollection services)
        {
            services.AddOptions<AmazonSqsTransportOptions>().BindConfiguration("Unbound");
        }
    }
}
