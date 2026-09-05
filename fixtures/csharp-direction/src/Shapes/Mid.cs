// Middle of the Root -> Mid -> Leaf hierarchy. Overrides Overridden and ReOverridden (Leaf
// re-overrides ReOverridden only), hides Hidden with `new` (no polymorphism through a
// Root-typed receiver), and overrides OverriddenProp. MidOnly is declared only here, one hop
// above Leaf, to probe a bare call resolving through an intermediate base.
namespace Fixture.Shapes;

public class Mid : Root
{
    public override void Overridden() { }

    public override void ReOverridden() { }

    public new void Hidden() { }

    public override int OverriddenProp => 2;

    public void MidOnly() { }
}
