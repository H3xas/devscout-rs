// Derives from Stamper AND explicitly implements IStamp.Stamp(): a `this.Stamp()`, a bare
// `Stamp()` and a `base.Stamp()` inside this type all bind Stamper's public member (the explicit
// implementation is reachable only through an IStamp-typed receiver).
namespace Fixture.Shapes;

public class MixedExplicit : Stamper, IStamp
{
    void IStamp.Stamp() { }

    public void Probe()
    {
        // case X1
        this.Stamp();
        // case X2
        base.Stamp();
        // case X3
        Stamp();
    }
}
