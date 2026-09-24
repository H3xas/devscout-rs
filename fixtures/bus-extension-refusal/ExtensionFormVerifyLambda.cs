namespace BusExtensionRefusal
{
    public class SignalProbe
    {
        public void Arrange(IWatchBus bus)
        {
            bus.VerifiedOnce(x => x.Publish<Signal>(Any.Of<Signal>()));
            bus.NeverCalled(x => x.Publish<Signal>(Any.Of<Signal>()));
        }
    }
}
