// Case h: Touch (protected) and Stamp (public) are declared here; Shipment.cs, a different
// file, calls both through `base.` -- probes the base-member lookup for a protected member
// (any-visibility) alongside an ordinary public one.
// Case j: Root is a protected field declared here and used bare, with no qualifier, from the
// derived Shipment class -- probes the cross-file field-typing table walked across an
// in-graph base (see Order.Previous, case i, for the same table on a partial sibling instead).
namespace Fixture.Domain;

public class AuditableEntity
{
    protected Order Root = null!;

    protected void Touch() { }

    public void Stamp() { }
}
