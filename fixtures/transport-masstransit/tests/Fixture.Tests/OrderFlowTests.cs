using System.Threading.Tasks;
using MassTransit;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.Extensions.DependencyInjection;
using Fixture.Service.Consumers;
using Xunit;

namespace Fixture.Tests
{
    public class OrderFlowTests
    {
        [Fact]
        public async Task Submits()
        {
            await using var application = new WebApplicationFactory<Program>()
                .WithWebHostBuilder(builder => builder.ConfigureServices(services =>
                    services.AddMassTransitTestHarness(x => x.AddConsumer<SubmitOrderConsumer>())));
        }
    }
}
