// Base interface implemented, extended and hidden-behind-explicit-implementation by the
// classes in Implicit.cs, Explicit.cs, Both.cs and Inheriting.cs.
namespace Fixture.Shapes;

public interface IContract
{
    void Fulfil();

    int Size { get; }
}
