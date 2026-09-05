// Root of a three-level class hierarchy (Root -> Mid -> Leaf) used to probe how a resolver
// walks `this.`, `base.` and bare member references up and down a base chain.
// Inherited: never overridden anywhere in the hierarchy.
// Overridden: virtual here, overridden once in Mid, not touched again in Leaf.
// ReOverridden: virtual here, overridden in both Mid and Leaf.
// Hidden: non-virtual here, hidden (not overridden) with `new` in Mid.
// StaticInherited: static, called through the derived type name in Driver.cs.
// InheritedProp: a property never overridden anywhere.
// OverriddenProp: a virtual property overridden once in Mid.
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
