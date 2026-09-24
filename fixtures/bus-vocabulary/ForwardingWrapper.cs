using System.Threading.Tasks;

namespace BusVocabulary
{
    public class DispatchDesk
    {
        private readonly IShelfBus _bus;

        public DispatchDesk(IShelfBus bus) => _bus = bus;

        public Task Forward<TNotice>(TNotice notice) where TNotice : class =>
            _bus.Publish<TNotice>(notice);
    }

    public class WrappedPublisher
    {
        private readonly DispatchDesk _desk;

        public WrappedPublisher(DispatchDesk desk) => _desk = desk;

        public Task Announce() => _desk.Forward(new ReturnsDeskNotice { Aisle = "3A" });
    }

    public class SecondHandDesk
    {
        private readonly DispatchDesk _desk;

        public SecondHandDesk(DispatchDesk desk) => _desk = desk;

        public Task Relay<TNotice>(TNotice notice) where TNotice : class =>
            _desk.Forward<TNotice>(notice);
    }

    public class TwiceWrappedPublisher
    {
        private readonly SecondHandDesk _desk;

        public TwiceWrappedPublisher(SecondHandDesk desk) => _desk = desk;

        public Task Announce() => _desk.Relay(new OverdueNotice { DaysLate = 2 });
    }
}
