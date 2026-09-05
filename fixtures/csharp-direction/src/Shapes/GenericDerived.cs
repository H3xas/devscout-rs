// A generic class deriving from the non-generic Root, used only as a static qualifier
// (`GenericDerived<int>.StaticInherited()`): the type-argument list marks the qualifier as a type
// with certainty, and the static member it names is declared on Root.
namespace Fixture.Shapes;

public class GenericDerived<T> : Root
{
    public T Item = default!;
}
