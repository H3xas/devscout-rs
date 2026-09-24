using System.Threading;
using System.Threading.Tasks;

namespace BusArrayIdentity
{
    public interface IWatchBus
    {
        Task Publish<TMessage>(TMessage message, CancellationToken ct = default) where TMessage : class;

        Task Send(object message, CancellationToken ct = default);
    }

    public interface IConsumer<TMessage>
    {
        Task Consume(TMessage message);
    }
}
