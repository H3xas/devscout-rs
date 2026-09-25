using System.Threading.Tasks;

namespace BusVocabularyAdmission
{
    public class PilotPublisher
    {
        private readonly IPilotBus _bus;

        public PilotPublisher(IPilotBus bus) => _bus = bus;

        public Task Announce() => _bus.Publish(new PilotRequest { Vessel = "Meridian" });
    }

    public class BerthPublisher
    {
        private readonly IPilotBus _bus;

        public BerthPublisher(IPilotBus bus) => _bus = bus;

        public Task Announce() => _bus.Publish(new BerthNotice { Berth = "12" });
    }

    public class TidePublisher
    {
        private readonly IPilotBus _bus;

        public TidePublisher(IPilotBus bus) => _bus = bus;

        public Task Announce() => _bus.Publish(new TideNotice { HeightCm = 340 });
    }

    public class EchoPublisher
    {
        private readonly IPilotBus _bus;

        public EchoPublisher(IPilotBus bus) => _bus = bus;

        public Task Announce() => _bus.Publish(new EchoNotice { Sounding = "clear" });
    }

    public class SelfCarriedPublisher
    {
        private readonly IPilotBus _bus;

        public SelfCarriedPublisher(IPilotBus bus) => _bus = bus;

        public Task Announce() => _bus.Publish(new SelfCarriedNotice { Origin = "sweep" });
    }
}
