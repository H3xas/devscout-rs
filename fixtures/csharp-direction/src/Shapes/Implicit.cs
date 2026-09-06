// Implicit implementation of IContract: the class members are public and directly satisfy
// the interface, so both a class-typed and an interface-typed receiver bind the same
// declaration here.
namespace Fixture.Shapes;

public class Implicit : IContract
{
    public void Fulfil() { }

    public int Size => 0;
}
