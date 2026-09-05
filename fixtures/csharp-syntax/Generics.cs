// Declarations: same-name generic/non-generic class, generic class with multiple constraints, contravariant/covariant interfaces, generic delegate, generic methods with unmanaged/notnull/self-referential/Enum constraints.
namespace Syntax.Generics;

public class GenericBox
{
    public int Marker = 1;
}

public class GenericBox<T> where T : class, new()
{
    public T Create() => new T();
}

public class GenericPair<TKey, TValue> : GenericBox<TKey>
    where TKey : class, new()
    where TValue : struct
{
    public TValue Value;
}

public interface IGenericRepo<in T>
{
    void Store(T item);
}

public interface IGenericSource<out T>
{
    T Fetch();
}

public delegate TOut GenericMap<in TIn, out TOut>(TIn input);

public class GenericItem
{
}

public class GenericRepoImpl<T> : IGenericRepo<T>
{
    public void Store(T item)
    {
    }
}

public class GenericSelfRepo : IGenericRepo<GenericSelfRepo>
{
    public void Store(GenericSelfRepo item)
    {
    }
}

public class GenericSourceImpl<T> : IGenericSource<T> where T : new()
{
    public T Fetch() => new T();
}

public enum GenericColor
{
    Red,
    Green,
}

public static class GenericMethods
{
    public static TResult Convert<TResult>(object value) where TResult : struct => (TResult)value;

    public static void UseUnmanaged<T>(T value) where T : unmanaged
    {
    }

    public static void UseNotNull<T>(T value) where T : notnull
    {
    }

    public static void UseSelfReferential<T>(T repo) where T : IGenericRepo<T>
    {
    }

    public static void UseEnum<T>(T value) where T : Enum
    {
    }
}

public class GenericUser
{
    public void Run()
    {
        var box = new GenericBox();
        var typedBox = new GenericBox<GenericItem>();
        var nestedBox = new GenericBox<GenericBox<GenericItem>>();
        var created = typedBox.Create();
        var pair = new GenericPair<GenericItem, int> { Value = 3 };
        IGenericRepo<string> repo = new GenericRepoImpl<string>();
        repo.Store("x");
        IGenericSource<GenericItem> source = new GenericSourceImpl<GenericItem>();
        IGenericSource<object> covariantSource = source;
        var fetched = covariantSource.Fetch();
        GenericMap<int, string> map = x => x.ToString();
        var mapped = map(1);
        int explicitResult = GenericMethods.Convert<int>(1);
        GenericMethods.UseUnmanaged(1);
        GenericMethods.UseNotNull("a");
        GenericMethods.UseSelfReferential(new GenericSelfRepo());
        GenericMethods.UseEnum(GenericColor.Red);
        Console.WriteLine($"{box.Marker}{created}{pair.Value}{mapped}{explicitResult}{fetched}{nestedBox}");
    }
}
