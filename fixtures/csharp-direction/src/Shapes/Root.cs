namespace Fixture.Shapes;

public class Root
{
    public virtual void Inherited() { }

    public virtual void Overridden() { }

    public virtual void ReOverridden() { }

    public void Hidden() { }

    public static void StaticInherited() { }

    public int InheritedProp => 1;

    public virtual int OverriddenProp => 1;
}
