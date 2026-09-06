namespace Fixture.Domain;

public class AuditableEntity
{
    protected Order Root = null!;

    protected void Touch() { }

    public void Stamp() { }
}
