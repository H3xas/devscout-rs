// References: typeof, nameof, sizeof, default, casts, as/is, checked/unchecked.
namespace Syntax.TypeOps;

class TypeOpGeneric<T>
{
    public T? Item;
}

class TypeOpTarget
{
    public string? Name;
    public int Value;
}

class TypeOpUser
{
    public void Run(object o)
    {
        _ = typeof(TypeOpTarget);
        _ = typeof(List<>);
        _ = typeof(TypeOpGeneric<>);
        _ = typeof(TypeOpGeneric<int>);
        _ = typeof(Dictionary<,>);
        _ = typeof(TypeOpTarget[]);
        _ = nameof(TypeOpTarget);
        _ = nameof(TypeOpTarget.Name);
        _ = nameof(o);
        _ = sizeof(int);
        _ = default(TypeOpTarget);
        TypeOpTarget? t = default;
        _ = (TypeOpTarget)o;
        _ = (int)3.5;
        _ = o as TypeOpTarget;
        _ = o is TypeOpTarget;

        if (o is TypeOpTarget tt)
        {
            _ = tt.Value;
        }

        _ = o is not null;
        _ = checked((int)1L);
        _ = unchecked((int)1L);
        _ = (TypeOpTarget?)null;
        _ = (List<TypeOpTarget>)o;
        dynamic dyn = o;
        _ = (dynamic)o;
        _ = ((int, string))o;
        _ = (o as TypeOpTarget)?.Name;
        _ = ((TypeOpTarget)o).Value;
        var cast = (TypeOpTarget)o;
        _ = cast.Value;

        _ = t;
        _ = dyn;
    }
}
