using Fixtures.Enrichment.Widgets;

namespace Fixtures.Enrichment
{
    public abstract class Handler<T>
    {
        private T current;

        protected Handler(T current)
        {
            this.current = current;
        }

        public T Get()
        {
            return current;
        }
    }

    public class WidgetHandler : Handler<BetaWidget>
    {
        public WidgetHandler(BetaWidget widget)
            : base(widget)
        {
        }
    }
}
