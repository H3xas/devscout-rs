using System;

namespace BusNonPublicOverload
{
    public class RelayStation
    {
        private readonly IRelayBus _bus;

        public RelayStation(IRelayBus bus)
        {
            _bus = bus;
        }

        private void Setup(Action<IRelayBus> configure)
        {
            configure(_bus);
        }
    }
}
