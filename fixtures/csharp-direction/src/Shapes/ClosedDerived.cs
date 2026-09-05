// Closes GenericBase<T> at int, with no members of its own -- every probe against it resolves
// up to the open generic declaration.
namespace Fixture.Shapes;

public class ClosedDerived : GenericBase<int>
{
}
