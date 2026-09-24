namespace BusOptionalAlignment
{
    public class ExtensionPlainDelegateProbe
    {
        public void Arrange(ISignalBus bus)
        {
            bus.RunsLive(x => x.Publish<Beacon>(Probe.Any<Beacon>()));
        }
    }
}
