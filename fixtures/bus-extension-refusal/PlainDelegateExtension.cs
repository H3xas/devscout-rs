namespace BusExtensionRefusal
{
    public class SignalRelay
    {
        public void Arrange(IWatchBus bus)
        {
            bus.RunsDirectly(x => x.Publish<Signal>(Any.Of<Signal>()));
        }
    }
}
