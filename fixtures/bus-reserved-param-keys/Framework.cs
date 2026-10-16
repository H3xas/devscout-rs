using System.Threading.Tasks;

namespace BusReservedParamKeys
{
    public interface IShoalBus
    {
        Task Publish<TNotice>(TNotice notice) where TNotice : class;
    }

    public interface IShoalRegistry
    {
        IShoalRegistry AddShoalIndexer<TIndexer>();
        IShoalRegistry AddReefGuard<TGuard>();
        IShoalRegistry AddCurrentLogger<TLogger>();
    }

    public abstract class ShoalBase<TNotice>
    {
        public abstract Task Fetch();
    }

    public abstract class ReefBase<TNotice>
    {
        public abstract Task Fetch();
    }

    public abstract class CurrentBase<TNotice>
    {
        public abstract Task Fetch();
    }
}
