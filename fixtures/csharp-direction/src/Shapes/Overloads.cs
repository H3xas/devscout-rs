// Same-arity overloads split across a base and a derived type by parameter TYPE: the derived
// M(string) is not applicable to an int argument, so the compiler binds the base's M(int) even
// though the derived type declares a same-named, same-arity member.
namespace Fixture.Shapes;

public class OverloadBase
{
    public void Same(int value) { }
}

public class OverloadDerived : OverloadBase
{
    public void Same(string value) { }
}
