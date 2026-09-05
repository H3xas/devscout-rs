// A `private new` field with the same name as Root's public InheritedProp: from outside this type
// the private field is inaccessible, so a ShadowField-typed receiver reading InheritedProp binds
// Root's property.
namespace Fixture.Shapes;

public class ShadowField : Root
{
    private new int InheritedProp = 3;

    public int Peek() => InheritedProp;
}
