using System.Threading.Tasks;

namespace BusRegistrationMorphology
{
    public class EditionPublisher
    {
        private readonly IPressBus _bus;

        public EditionPublisher(IPressBus bus) => _bus = bus;

        public Task Announce() => _bus.Publish(new EditionNotice { Imprint = "first" });
    }
}
