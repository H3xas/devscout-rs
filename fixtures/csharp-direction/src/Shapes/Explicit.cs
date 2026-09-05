// Explicit implementation of IContract: Fulfil and Size are only reachable through an
// IContract-typed receiver (or a cast), never through this class's own type -- Own is the
// control member reachable directly.
namespace Fixture.Shapes;

public class Explicit : IContract
{
    void IContract.Fulfil() { }

    int IContract.Size => 0;

    public void Own() { }
}
