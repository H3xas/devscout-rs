// Declared return type Root, actual instance Leaf: a chain tail on MakeRoot() binds Root's
// declaration of Overridden, because the static type of the call is Root.
namespace Fixture.Shapes;

public static class Factory
{
    public static Root MakeRoot() => new Leaf();
}
