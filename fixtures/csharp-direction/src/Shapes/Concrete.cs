// Implements AbstractBase's abstract member. Probe() adds a base-qualified call to the
// inherited concrete member (case C5) and a this-qualified call to this type's own override
// (case B4), alongside the this/base cases already covered by Leaf.cs's hierarchy.
namespace Fixture.Shapes;

public class Concrete : AbstractBase
{
    public override void Implement() { }

    public void Probe()
    {
        // case C5
        base.Concrete();
        // case B4
        this.Implement();
    }
}
