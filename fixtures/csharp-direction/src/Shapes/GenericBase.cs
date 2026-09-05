// The one generic type in this fixture, closed by ClosedDerived.cs. Generic arity is owned
// by a different fixture; this case only probes a member declared on an open generic base
// reached through a closed derived receiver.
namespace Fixture.Shapes;

public class GenericBase<T>
{
    public T Value = default!;

    public void Store(T item) { }
}
