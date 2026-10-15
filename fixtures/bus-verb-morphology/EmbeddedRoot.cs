namespace HouseDispatch
{
    public class EmbeddedRoot
    {
        private readonly IParcelBus _bus;
        public EmbeddedRoot(IParcelBus bus) => _bus = bus;

        public void Calls()
        {
            QuietNotice notice = new QuietNotice();
            _bus.Published(notice);
            _bus.Sender(notice);
            _bus.Dispatcher(notice);
            _bus.Postgres(notice);
        }
    }
}
