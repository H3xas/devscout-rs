using System.Threading.Tasks;

namespace HouseDispatch
{
    public class ParcelNotice { }
    public class QuietNotice { }

    public class ParcelWorker : ParcelWorkerBase<ParcelNotice>
    {
        public override Task Work(ParcelNotice message) => Task.CompletedTask;
    }

    public class QuietWorker : ParcelWorkerBase<QuietNotice>
    {
        public override Task Work(QuietNotice message) => Task.CompletedTask;
    }

    public static class Installation
    {
        public static void Install(IParcelRegistry registry)
        {
            registry.AddParcelWorker<ParcelWorker>();
            registry.AddParcelWorker<QuietWorker>();
        }
    }
}
