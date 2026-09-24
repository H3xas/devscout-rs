using System;
using System.Threading;
using System.Threading.Tasks;

namespace BusVocabulary
{
    public interface IShelfBus
    {
        Task Publish<TNotice>(TNotice notice, CancellationToken ct = default) where TNotice : class;
        Task Enqueue<TNotice>(TNotice notice, CancellationToken ct = default) where TNotice : class;
    }

    public abstract class ShelfWorkerBase<TNotice>
    {
        public abstract Task Work(TNotice notice);
    }

    public interface IBindingHandler<TNotice>
    {
        Task Accept(TNotice notice);
    }

    public class Chime<TNotice>
    {
        public TNotice Last { get; set; }
    }

    public interface IWorkshopRegistry
    {
        IWorkshopRegistry AddShelfWorker<TWorker>();
        IWorkshopRegistry AddBindingHandler<THandler>();
    }
}
