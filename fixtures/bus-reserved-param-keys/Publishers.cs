using System.Threading.Tasks;

namespace BusReservedParamKeys
{
    public class ShoalPublisher
    {
        private readonly IShoalBus _bus;

        public ShoalPublisher(IShoalBus bus) => _bus = bus;

        public Task Announce() => _bus.Publish(new ShoalNotice { Shoal = "north" });
    }

    public class ReefPublisher
    {
        private readonly IShoalBus _bus;

        public ReefPublisher(IShoalBus bus) => _bus = bus;

        public Task Announce() => _bus.Publish(new ReefNotice { Reef = "outer" });
    }

    public class CurrentPublisher
    {
        private readonly IShoalBus _bus;

        public CurrentPublisher(IShoalBus bus) => _bus = bus;

        public Task Announce() => _bus.Publish(new CurrentNotice { SpeedKnots = 3 });
    }
}
