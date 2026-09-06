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
