// Declarations: record primary constructor, derived record calling base primary constructor, class primary constructor capturing a parameter, struct primary constructor, base/derived class primary constructors.
namespace Syntax.Primary;

public record PrimaryRecord(string Name, int Age);

public record DerivedPrimary(string Name) : PrimaryRecord(Name, 0);

public class PrimaryDependency
{
    public string Describe() => "d";
}

public class PrimaryService(PrimaryDependency dependency)
{
    public string Describe() => dependency.Describe();
}

public struct PrimaryPoint(int x, int y)
{
    public int Sum => x + y;
}

public class PrimaryBase(int id)
{
    public int Id => id;
}

public class PrimaryChild(int id, string tag) : PrimaryBase(id)
{
    public string Tag => tag;
}

public class PrimaryUser
{
    public void Run()
    {
        var record = new PrimaryRecord("a", 1);
        _ = record.Age;
        var derived = new DerivedPrimary("b");
        var dependency = new PrimaryDependency();
        var service = new PrimaryService(dependency);
        var point = new PrimaryPoint(1, 2);
        var baseInstance = new PrimaryBase(1);
        var child = new PrimaryChild(2, "x");
        Console.WriteLine($"{record}{derived}{service.Describe()}{point.Sum}{baseInstance.Id}{child.Tag}{child.Id}");
    }
}
