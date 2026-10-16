using System.Collections.Generic;
using System.Threading.Tasks;

namespace BusReservedParamKeys
{
    public class ShoalIndexer : ShoalBase<ShoalNotice>
    {
        public override Task Fetch() => Task.CompletedTask;

        public Dictionary<ShoalNotice, TKey> Index<TKey>() => new Dictionary<ShoalNotice, TKey>();
    }

    public class ReefGuard : ReefBase<ReefNotice>
    {
        public override Task Fetch() => Task.CompletedTask;

        public Task @event(ReefNotice notice) => Task.CompletedTask;
    }

    public class CurrentLogger : CurrentBase<CurrentNotice>
    {
        public override Task Fetch() => Task.CompletedTask;

        public Dictionary<CurrentNotice, TKey> Index<TKey>() => new Dictionary<CurrentNotice, TKey>();

        public Task Log(CurrentNotice notice) => Task.CompletedTask;
    }

    public static class Installation
    {
        public static void Install(IShoalRegistry registry)
        {
            registry.AddShoalIndexer<ShoalIndexer>();
            registry.AddReefGuard<ReefGuard>();
            registry.AddCurrentLogger<CurrentLogger>();
        }
    }
}
