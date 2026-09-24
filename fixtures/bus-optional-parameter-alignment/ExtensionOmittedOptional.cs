namespace BusOptionalAlignment
{
    public class ExtensionOmittedProbe
    {
        public void Arrange(ISignalBus bus)
        {
            bus.Confirmed(x => x.Publish<Beacon>(Probe.Any<Beacon>()));
        }
    }
}
