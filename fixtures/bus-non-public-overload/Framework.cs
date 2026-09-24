using System.Threading;
using System.Threading.Tasks;

namespace BusNonPublicOverload
{
    public interface IRelayBus
    {
        Task Publish<TMessage>(TMessage message, CancellationToken ct = default) where TMessage : class;
    }

    public interface IConsumer<TMessage>
    {
        Task Consume(TMessage message);
    }

    public static class Match
    {
        public static T Any<T>() => default;
    }
}
