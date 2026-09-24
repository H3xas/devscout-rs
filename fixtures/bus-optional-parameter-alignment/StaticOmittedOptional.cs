namespace BusOptionalAlignment
{
    public class StaticOmittedProbe
    {
        public void Arrange(ISignalBus bus)
        {
            CheckHelpers.ConfirmedStatic(bus, x => x.Publish<Beacon>(Probe.Any<Beacon>()));
        }
    }
}
