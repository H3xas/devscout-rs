using System.Collections.Generic;
using System.Threading.Tasks;

namespace BusVocabularyAdmission
{
    public class PilotOfficer : PilotHandlerBase<PilotRequest>
    {
        public override Task Handle(PilotRequest notice) => Task.CompletedTask;
    }

    public class BerthGuard : BerthWatcherBase<BerthNotice>
    {
        public override Task Watch(BerthNotice notice) => Task.CompletedTask;
    }

    public class TerminalWatcher : TerminalBase<TerminalNotice>
    {
        public override Task Arrive(TerminalNotice notice) => Task.CompletedTask;
    }

    public class SilentWatcher : SilentBase<TideNotice>
    {
        public override Task<TideNotice> Fetch() => Task.FromResult(new TideNotice());
    }

    public class EchoWatcher : ChimeWatcherBase<TideNotice>
    {
        public override Task Watch() => Task.CompletedTask;

        public Chime<EchoNotice> Signal { get; set; }
    }

    public class BerthBatchHandler : WrappedHandlerBase<BerthNotice>
    {
        public override Task HandleBatch(List<BerthNotice> notices) => Task.CompletedTask;
    }

    public static class Installation
    {
        public static void Install(IPilotRegistry registry)
        {
            registry.AddPilotHandler<PilotOfficer>();
            registry.AddPilotHandler<PilotRequest>();
            registry.AddBerthWatcher<BerthGuard>();
            registry.AddTerminalWatcher<TerminalWatcher>();
            registry.AddSilentWatcher<SilentWatcher>();
            registry.AddChimeWatcher<EchoWatcher>();
            registry.AddWrappedHandler<BerthBatchHandler>();
            registry.AddLedgerReader<SelfCarriedNotice>();
        }
    }
}
