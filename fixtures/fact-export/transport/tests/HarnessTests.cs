using MassTransit;
using Microsoft.Extensions.DependencyInjection;
using Shop.Transport;
using Xunit;

namespace Shop.Transport.Tests
{
    public class HarnessTests
    {
        [Fact]
        public void Substitutes()
        {
            new ServiceCollection().AddMassTransitTestHarness(x => x.AddConsumer<IntakeConsumer>());
        }
    }
}
