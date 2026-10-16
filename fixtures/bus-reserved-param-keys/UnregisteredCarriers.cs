using System.Threading.Tasks;

namespace BusReservedParamKeys
{
    public class ReefShoalWatcher : ShoalBase<ReefNotice>
    {
        public override Task Fetch() => Task.CompletedTask;

        public Task Watch(ReefNotice notice) => Task.CompletedTask;
    }
}
