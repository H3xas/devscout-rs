using System.Collections.Generic;
using System.Threading.Tasks;

namespace BusVocabularyAdmission
{
    public interface IPilotBus
    {
        Task Publish<TNotice>(TNotice notice) where TNotice : class;
    }

    public interface IPilotRegistry
    {
        IPilotRegistry AddPilotHandler<THandler>();
        IPilotRegistry AddBerthWatcher<TWatcher>();
        IPilotRegistry AddTerminalWatcher<TWatcher>();
        IPilotRegistry AddSilentWatcher<TWatcher>();
        IPilotRegistry AddChimeWatcher<TWatcher>();
        IPilotRegistry AddWrappedHandler<THandler>();
        IPilotRegistry AddLedgerReader<TReader>();
    }

    public abstract class PilotHandlerBase<TNotice>
    {
        public abstract Task Handle(TNotice notice);
    }

    public abstract class BerthWatcherBase<TNotice>
    {
        public abstract Task Watch(TNotice notice);
    }

    public abstract class TerminalBase<TNotice>
    {
        public abstract Task Arrive(TNotice notice);
    }

    public abstract class SilentBase<TNotice>
    {
        public abstract Task<TNotice> Fetch();
    }

    public abstract class ChimeWatcherBase<TNotice>
    {
        public abstract Task Watch();
    }

    public abstract class WrappedHandlerBase<TNotice>
    {
        public abstract Task HandleBatch(List<TNotice> notices);
    }

    public abstract class LedgerBase<TNotice>
    {
        public abstract Task Read(TNotice notice);
    }

    public class Chime<TNotice>
    {
        public TNotice Last { get; set; }
    }
}
