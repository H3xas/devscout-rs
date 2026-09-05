// Declared-type-versus-initializer receivers: a field, an auto-property, a constructor-assigned
// field and a constructor parameter each declare a base or interface type while holding a more
// derived instance. Every member access binds the DECLARED type's member (or the first base of
// that declared type that declares it), never the instance's.
namespace Fixture.Shapes;

public class Holder
{
    private readonly IContract _initialised = new Implicit();
    private readonly Root _assignedInCtor;
    private readonly IContract _injected;

    public Root Item { get; } = new Leaf();

    public Holder(IContract injected)
    {
        _assignedInCtor = new Leaf();
        _injected = injected;
    }

    public void Probe()
    {
        // case H2
        _initialised.Fulfil();
        // case H3
        _assignedInCtor.Overridden();
        // case H4
        _injected.Fulfil();
        // case H7
        Item.Overridden();
        // case H8
        _assignedInCtor.Inherited();
    }
}
