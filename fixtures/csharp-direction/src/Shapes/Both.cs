// Implements IExtended (and transitively IContract) with implicit implementations of all
// three members, so a receiver typed to either interface, or to IContract held via the
// extended interface, all bind these same declarations.
namespace Fixture.Shapes;

public class Both : IExtended
{
    public void Fulfil() { }

    public int Size => 0;

    public void Extra() { }
}
