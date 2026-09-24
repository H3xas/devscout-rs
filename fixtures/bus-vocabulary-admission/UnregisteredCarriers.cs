using System.Threading.Tasks;

namespace BusVocabularyAdmission
{
    public class LedgerReader : LedgerBase<BerthNotice>
    {
        public override Task Read(BerthNotice notice) => Task.CompletedTask;
    }

    public class SecondTerminalWatcher : TerminalBase<BerthNotice>
    {
        public override Task Arrive(BerthNotice notice) => Task.CompletedTask;
    }

    public class SecondSilentWatcher : SilentBase<BerthNotice>
    {
        public override Task<BerthNotice> Fetch() => Task.FromResult(new BerthNotice());
    }
}
