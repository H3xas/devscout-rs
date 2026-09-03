// Case h/j: Close() calls the AuditableEntity base (src/Domain/AuditableEntity.cs) through
// `base.Touch()` (protected) and `base.Stamp()` (public), then reads Root, a protected field
// declared on that same base, bare.
namespace Fixture.Domain;

public class Shipment : AuditableEntity
{
    public bool Close()
    {
        base.Touch();
        base.Stamp();
        return Root.Validate();
    }
}
