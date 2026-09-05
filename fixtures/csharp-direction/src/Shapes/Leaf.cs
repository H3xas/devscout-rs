// Bottom of the Root -> Mid -> Leaf hierarchy. Re-overrides ReOverridden (Mid's override is
// skipped by any call not routed through `base.`). Probe() exercises this-qualified,
// base-qualified and bare member references against this same hierarchy from inside the most
// derived type, one probe site per source line.
namespace Fixture.Shapes;

public class Leaf : Mid
{
    public override void ReOverridden() { }

    public void Probe()
    {
        // case B1
        this.Inherited();
        // case B2
        this.Overridden();
        // case B3
        this.ReOverridden();
        // case C1
        base.Inherited();
        // case C2
        base.Overridden();
        // case C3
        base.ReOverridden();
        // case C4
        base.Hidden();
        // case D1
        Inherited();
        // case D2
        Overridden();
        // case D3
        MidOnly();
    }
}
