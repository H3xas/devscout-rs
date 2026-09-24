using System.Threading;
using System.Threading.Tasks;

namespace BusOptionalAlignment
{
    public interface ISignalBus
    {
        Task Publish<TMessage>(TMessage message, CancellationToken ct = default) where TMessage : class;
    }

    public interface IConsumer<TMessage>
    {
        Task Consume(TMessage message);
    }

    public static class Probe
    {
        public static T Any<T>() => default;
    }
}
