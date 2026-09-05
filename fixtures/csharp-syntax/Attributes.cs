// References: assembly, type, method, parameter, return, field, property, event, enum-member, generic and type-parameter attributes.
[assembly: Syntax.Attrs.Marker("asm")]

namespace Syntax.Attrs;

[AttributeUsage(AttributeTargets.All, AllowMultiple = true)]
class MarkerAttribute : Attribute
{
    public MarkerAttribute(string name)
    {
    }

    public MarkerAttribute(Type type)
    {
    }

    public int Level { get; set; }
}

class GenericMarkerAttribute<T> : Attribute
{
}

enum AttrsKind
{
    [Marker("enum")]
    First,
    Second,
}

[Marker("type", Level = 1)]
[Marker("a"), Marker("b")]
class AttrsHost
{
    [Marker("field")]
    public int FieldValue;

    [field: Marker("backing")]
    public int AutoProp { get; set; }

    [Marker("prop")]
    public int PropValue { get; set; }

    [method: Marker("evt")]
    public event EventHandler? Changed;

    [Marker("method")]
    [return: Marker("ret")]
    public int Compute([Marker("param")] int input)
    {
        return input;
    }

    [Obsolete("x")]
    public void Legacy()
    {
    }

    [System.Diagnostics.Conditional("DEBUG")]
    public void Trace()
    {
    }

    [Marker(typeof(MarkerAttribute))]
    public void ByType()
    {
    }

    [Marker(nameof(MarkerAttribute))]
    public void ByName()
    {
    }

    [GenericMarker<int>]
    public void Generic()
    {
    }

    public int WithLocal()
    {
        [Marker("local")]
        static int Helper(int x) => x;

        return Helper(1);
    }

    public Func<int, int> Lambda()
    {
        return [Marker("lambda")] (int x) => x;
    }
}

class AttrsGenericHost<[Marker("tp")] T>
{
    public T? Value;
}
