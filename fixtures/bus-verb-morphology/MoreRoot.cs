using System.Threading.Tasks;

namespace HouseDispatch
{
    public class MoreRoot
    {
        private readonly IParcelBus _bus;
        public MoreRoot(IParcelBus bus) => _bus = bus;

        public Task Generic() => _bus.EnqueueMessages<ParcelNotice>(default);
        public Task Constructed() => _bus.EnqueueMessages(new ParcelNotice());
        public Task Identifier()
        {
            ParcelNotice notice = new ParcelNotice();
            return _bus.EnqueueMessages(notice);
        }
    }
}
