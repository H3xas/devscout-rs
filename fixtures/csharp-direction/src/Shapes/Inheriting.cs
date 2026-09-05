// Inherits Implicit's implementation of IContract without redeclaring it, to probe a call
// through a derived class reaching an interface member whose implementation lives two hops
// up (Implicit implements the interface, Inheriting only adds its own member).
namespace Fixture.Shapes;

public class Inheriting : Implicit
{
    public void More() { }
}
