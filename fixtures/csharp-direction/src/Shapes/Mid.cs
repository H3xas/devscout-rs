namespace Fixture.Shapes;

public class Mid : Root
{
    public override void Overridden() { }

    public override void ReOverridden() { }

    public new void Hidden() { }

    public override int OverriddenProp => 2;

    public void MidOnly() { }
}
