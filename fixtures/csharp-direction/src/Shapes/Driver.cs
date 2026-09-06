namespace Fixture.Shapes;

public class Driver
{
    public void Run()
    {
        var root = new Root();
        var mid = new Mid();
        var leaf = new Leaf();
        Root asRoot = leaf;
        Mid asMid = leaf;
        var concrete = new Concrete();
        AbstractBase asAbstract = concrete;
        var implicitObj = new Implicit();
        IContract asContract = implicitObj;
        var explicitObj = new Explicit();
        IContract asContractExplicit = explicitObj;
        var both = new Both();
        IExtended asExtended = both;
        IContract asContractViaExtended = both;
        var inheriting = new Inheriting();
        var closed = new ClosedDerived();
        _ = root;

        // case A1
        mid.Inherited();
        // case A2
        leaf.Inherited();
        // case A3
        mid.Overridden();
        // case A4
        leaf.Overridden();
        // case A5
        leaf.ReOverridden();
        // case A6
        asRoot.Overridden();
        // case A7
        asMid.ReOverridden();
        // case A8
        mid.Hidden();
        // case A9
        leaf.Hidden();
        // case A10
        asRoot.Hidden();
        // case A11
        Mid.StaticInherited();
        // case A12
        _ = leaf.InheritedProp;
        // case A13
        _ = leaf.OverriddenProp;
        // case A14
        concrete.Implement();
        // case A15
        asAbstract.Implement();
        // case A16
        concrete.Concrete();
        // case E1
        asContract.Fulfil();
        // case E2
        implicitObj.Fulfil();
        // case E3
        asContractExplicit.Fulfil();
        // case E4
        explicitObj.Own();
        // case E5
        _ = asContract.Size;
        // case E6
        _ = implicitObj.Size;
        // case E7
        asExtended.Fulfil();
        // case E8
        asExtended.Extra();
        // case E9
        both.Fulfil();
        // case E10
        inheriting.Fulfil();
        // case E11
        asContractViaExtended.Fulfil();
        // case E12
        ((IContract)explicitObj).Fulfil();
        // case G1
        closed.Store(1);
        // case G2
        _ = closed.Value;
    }
}
