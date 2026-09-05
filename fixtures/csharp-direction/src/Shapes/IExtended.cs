// Extends IContract with one member of its own, to probe a receiver typed to the derived
// interface reaching a member declared on the base interface (Both.cs implements both).
namespace Fixture.Shapes;

public interface IExtended : IContract
{
    void Extra();
}
