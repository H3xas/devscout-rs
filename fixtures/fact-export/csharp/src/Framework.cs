using System.Threading.Tasks;

namespace Fixture.Bus
{
    public interface IBus
    {
        Task Publish<TNotice>(TNotice notice) where TNotice : class;
    }

    public abstract class NoticeHandlerBase<TNotice>
    {
        public abstract Task Handle(TNotice notice);
    }

    public interface IHandlerRegistry
    {
        IHandlerRegistry AddHandler<THandler>();
    }
}
