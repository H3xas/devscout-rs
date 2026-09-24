namespace BusExtensionRefusal
{
    public class SignalStaticProbe
    {
        public void Arrange(IWatchBus bus)
        {
            ProbeHelpers.VerifiedOnce(bus, x => x.Publish<Signal>(Any.Of<Signal>()));
        }
    }
}
