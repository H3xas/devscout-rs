// Declarations: interface default method, static abstract/virtual/field/method interface members, interface event/indexer/property, explicit interface implementations, generic math constraint.
namespace Syntax.Interfaces;

public interface IShapeContract
{
    double Area();

    double Perimeter() => 0;

    static abstract IShapeContract Create();

    static virtual string Kind => "shape";

    static int Counter = 0;

    static void Bump() => Counter++;

    event EventHandler? Resized;

    int this[int i] { get; }

    string Name { get; set; }
}

public class ShapeCircle : IShapeContract
{
    private string _name = "circle";

    double IShapeContract.Area() => 1;

    public double Area() => 2;

    string IShapeContract.Name
    {
        get => _name;
        set => _name = value;
    }

    int IShapeContract.this[int i] => i;

    event EventHandler? IShapeContract.Resized
    {
        add { }
        remove { }
    }

    public static IShapeContract Create() => new ShapeCircle();
}

public static class ShapeMath
{
    public static T Sum<T>(T a, T b) where T : System.Numerics.INumber<T> => a + b;

    public static TShape Make<TShape>() where TShape : IShapeContract => (TShape)TShape.Create();

    public static string DescribeKind<TShape>() where TShape : IShapeContract => TShape.Kind;
}

public class ShapeUser
{
    public void Run()
    {
        var circle = new ShapeCircle();
        var explicitArea = ((IShapeContract)circle).Area();
        var directArea = circle.Area();
        IShapeContract.Bump();
        var made = ShapeMath.Make<ShapeCircle>();
        var summed = ShapeMath.Sum(1, 2);
        var perimeter = ((IShapeContract)circle).Perimeter();
        var kind = ShapeMath.DescribeKind<ShapeCircle>();
        ((IShapeContract)circle).Resized += (_, _) => { };
        ((IShapeContract)circle).Name = "renamed";
        var name = ((IShapeContract)circle).Name;
        var indexed = ((IShapeContract)circle)[0];
        Console.WriteLine($"{explicitArea}{directArea}{IShapeContract.Counter}{made}{summed}{perimeter}{kind}{name}{indexed}");
    }
}
