namespace BusOptionalAlignment
{
    public class ExtensionSuppliedProbe
    {
        public void Arrange(ISignalBus bus)
        {
            bus.Confirmed(x => x.Publish<Beacon>(Probe.Any<Beacon>()), "checked");
        }
    }
}
