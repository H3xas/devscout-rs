// Second probe driver: declared-type-versus-initializer locals, chain tails, qualified and
// generic static qualifiers, accessibility and explicit-implementation shapes, and a same-arity
// overload split. One probe per source line, `// case` comment on the line above; the
// chain-tail line (case H6) yields two oracle records, the inner call and the tail.
namespace Fixture.Shapes;

public class Driver2
{
    public void Run()
    {
        Root asRootNew = new Leaf();
        var mixed = new MixedExplicit();
        IStamp asStamp = mixed;
        var deep = new Deep();
        var internalHider = new InternalHider();
        var internalDerived = new InternalDerived();
        var shadow = new ShadowField();
        var overload = new OverloadDerived();
        // case X17 (control: a member the receiver's own type declares)
        _ = shadow.Peek();

        // case H1
        asRootNew.Overridden();
        // case H6
        Factory.MakeRoot().Overridden();
        // case X5
        mixed.Stamp();
        // case X6
        asStamp.Stamp();
        // case X7
        deep.Stamp();
        // case X8
        internalHider.Hidden();
        // case X9
        internalDerived.Work();
        // case X10
        internalDerived.Pub();
        // case X11
        _ = shadow.InheritedProp;
        // case X13
        Fixture.Shapes.Mid.StaticInherited();
        // case X14
        GenericDerived<int>.StaticInherited();
        // case X15
        overload.Same(1);
        // case X16
        overload.Same("a");
    }
}
