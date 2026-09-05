// An `internal virtual` member (overridden in InternalDerived.cs) beside a public one that is
// only declared here.
namespace Fixture.Shapes;

public class InternalBase
{
    internal virtual void Work() { }

    public void Pub() { }
}
