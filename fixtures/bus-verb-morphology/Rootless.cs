namespace HouseDispatch
{
    public class Rootless
    {
        private readonly IParcelBus _bus;
        public Rootless(IParcelBus bus) => _bus = bus;

        public void Calls()
        {
            _bus.IsAny<QuietNotice>(default);
            _bus.ProjectTo<QuietNotice>(default);
        }
    }
}
