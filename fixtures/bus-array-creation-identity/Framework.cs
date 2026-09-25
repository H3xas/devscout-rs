using System.Threading;
using System.Threading.Tasks;

namespace BusArrayCreationIdentity
{
    public interface IWatchBus
    {
        Task Publish<TMessage>(TMessage message, CancellationToken ct = default) where TMessage : class;
    }

    public interface IConsumer<TMessage>
    {
        Task Consume(TMessage message);
    }
}
