using Fixtures.Enrichment.Widgets;

namespace Fixtures.Enrichment
{
    public abstract class Registry<T>
    {
        private readonly T current;

        protected Registry(T current)
        {
            this.current = current;
        }

        public T Current => current;
    }

    public class WidgetRegistry : Registry<BetaWidget>
    {
        public WidgetRegistry(BetaWidget widget)
            : base(widget)
        {
        }
    }
}
