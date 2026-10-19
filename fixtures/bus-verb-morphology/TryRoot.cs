using System.Threading.Tasks;

namespace HouseDispatch
{
    public class TryRoot
    {
        private readonly IParcelBus _bus;
        public TryRoot(IParcelBus bus) => _bus = bus;

        public Task Generic() => _bus.TryEmit<ParcelNotice>(default);
        public Task Constructed() => _bus.TryEmit(new ParcelNotice());
        public Task Identifier()
        {
            ParcelNotice notice = new ParcelNotice();
            return _bus.TryEmit(notice);
        }
    }
}
