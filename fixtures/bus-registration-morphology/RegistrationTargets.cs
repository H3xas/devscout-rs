using System.Threading.Tasks;

namespace BusRegistrationMorphology
{
    public class EditionBridge : PressMiddlewareBase<EditionNotice>
    {
        public override Task Invoke() => Task.CompletedTask;

        public Task Handle(EditionNotice notice) => Task.CompletedTask;
    }

    public class LedgerBridge : PressMiddlewareBase<LedgerNotice>
    {
        public override Task Invoke() => Task.CompletedTask;

        public Task Handle(LedgerNotice notice) => Task.CompletedTask;
    }

    public class QuietBridge : PressMiddlewareBase<EditionNotice>
    {
        public override Task Invoke() => Task.CompletedTask;
    }

    public class EagerBridge : PressMiddlewareBase<EditionNotice>
    {
        public override Task Invoke() => Task.CompletedTask;

        public Task Handle(EditionNotice notice) => Task.CompletedTask;
    }

    public class LooseHandler
    {
    }
}
