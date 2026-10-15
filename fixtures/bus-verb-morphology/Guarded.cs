namespace HouseDispatch
{
    public class Guarded
    {
        private readonly IParcelBus _bus;
        public Guarded(IParcelBus bus) => _bus = bus;

        public void Calls()
        {
            QuietNotice notice = new QuietNotice();
            _bus.GetPublish(notice);
            _bus.CancelPublish(notice);
            _bus.ConnectPublish(notice);
            _bus.CanPublish(notice);
            _bus.IsPublish(notice);
            _bus.HasPublish(notice);
            _bus.TryGetPublish(notice);
            _bus.TryCancelPublish(notice);
            _bus.TryConnectPublish(notice);
            _bus.TryCanPublish(notice);
            _bus.TryIsPublish(notice);
            _bus.TryHasPublish(notice);
        }
    }
}
