using System.Threading.Tasks;

namespace Fixture.Bus
{
    public class OrderPlacedHandler : NoticeHandlerBase<OrderPlaced>
    {
        public override Task Handle(OrderPlaced notice) => Task.CompletedTask;
    }

    public static class HandlerInstallation
    {
        public static void Install(IHandlerRegistry registry)
        {
            registry.AddHandler<OrderPlacedHandler>();
        }
    }

    public class OrderPublisher
    {
        private readonly IBus _bus;

        public OrderPublisher(IBus bus) => _bus = bus;

        public Task Announce() => _bus.Publish(new OrderPlaced { Id = 1 });

        public Task Ship() => _bus.Publish(new OrderShipped { Id = 2 });
    }
}
