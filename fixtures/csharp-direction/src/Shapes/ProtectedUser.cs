// A protected member reached through a receiver of the enclosing type itself (not `this`): C#
// admits the access from inside the declaring type, and binds this type's declaration.
namespace Fixture.Shapes;

public class ProtectedUser : Root
{
    protected void Guarded() { }

    public void Probe(ProtectedUser other)
    {
        // case X4
        other.Guarded();
    }
}
