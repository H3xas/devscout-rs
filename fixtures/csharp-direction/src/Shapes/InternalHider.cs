// Hides Root's public Hidden() with an `internal new` member: a same-assembly call through an
// InternalHider-typed receiver binds this type's member, not Root's.
namespace Fixture.Shapes;

public class InternalHider : Root
{
    internal new void Hidden() { }
}
