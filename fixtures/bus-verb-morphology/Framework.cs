using System.Threading.Tasks;

namespace HouseDispatch
{
    public interface IParcelBus
    {
        Task TryEmit<T>(T message);
        Task EnqueueMessages<T>(T message);
        Task GetPublish<T>(T message);
        Task CancelPublish<T>(T message);
        Task ConnectPublish<T>(T message);
        Task CanPublish<T>(T message);
        Task IsPublish<T>(T message);
        Task HasPublish<T>(T message);
        Task TryGetPublish<T>(T message);
        Task TryCancelPublish<T>(T message);
        Task TryConnectPublish<T>(T message);
        Task TryCanPublish<T>(T message);
        Task TryIsPublish<T>(T message);
        Task TryHasPublish<T>(T message);
        Task Published<T>(T message);
        Task Sender<T>(T message);
        Task Dispatcher<T>(T message);
        Task Postgres<T>(T message);
        Task IsAny<T>(T message);
        Task ProjectTo<T>(T message);
    }

    public abstract class ParcelWorkerBase<T>
    {
        public abstract Task Work(T message);
    }

    public interface IParcelRegistry
    {
        IParcelRegistry AddParcelWorker<T>();
    }
}
