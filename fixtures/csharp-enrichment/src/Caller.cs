using System;
using Fixtures.Enrichment.Alpha;
using Fixtures.Enrichment.Beta;
using Fixtures.Enrichment.Widgets;

namespace Fixtures.Enrichment
{
    public class Caller
    {
        public void SameContextOverride()
        {
            Config.Load();
        }

        public void NegativeCollision()
        {
            Config.Load();
        }

        public void LocalCallResult()
        {
            var factory = new Factory();
            var thing = factory.Get<BetaWidget>();
            thing.Render();
        }

        public void InheritedGenericCallback()
        {
            var handler = new WidgetHandler(new BetaWidget());
            var current = handler.Get();
            current.Render();
        }

        public void NestedTypedLambda()
        {
            var factory = new Factory();
            Action outer = () =>
            {
                var widget = factory.Get<BetaWidget>();
                Action inner = () => widget.Render();
                inner();
            };
            outer();
        }

        public void TypedIndexerResult()
        {
            var widgets = new Container<BetaWidget>(new[] { new BetaWidget() });
            widgets[0].Render();
        }

        public void QualifiedPropertyAccess()
        {
            var registry = new WidgetRegistry(new BetaWidget());
            registry.Current.Render();
        }

        public void SameLineDistinctFacts()
        {
            var factory = new Factory(); var thing = factory.Get<BetaWidget>(); var other = factory.Get<BetaWidget>(); thing.Render(); other.Paint();
        }

        public void OverloadedSameLine()
        {
            var options = new Options(); options.Configure(); options.Configure(true);
        }

        public void EqualArityOverloadTie()
        {
            var tie = new Tie();
            tie.Resolve(true);
        }
    }
}
