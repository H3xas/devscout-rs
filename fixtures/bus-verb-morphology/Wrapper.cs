using System.Threading.Tasks;

namespace HouseDispatch
{
    public class Wrapper
    {
        private readonly IParcelBus _bus;
        public Wrapper(IParcelBus bus) => _bus = bus;

        public Task Forward<T>(T message) => _bus.TryEmit(message);
        public Task ForwardCall() => Forward<QuietNotice>(new QuietNotice());
    }
}
