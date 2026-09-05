// Declarations: static class with extension methods (plain, generic, ref-returning), static-only class, extension methods on a user-defined type.
namespace Syntax.Statics;

public static class TextExtensions
{
    public static string Shout(this string s) => s.ToUpperInvariant();

    public static int Twice(this int n) => n * 2;

    public static T Pick<T>(this IEnumerable<T> items) => items.First();

    public static ref int RefFirst(this int[] xs) => ref xs[0];
}

public static class StaticOnly
{
    public static int Value;

    public static void Reset()
    {
        Value = 0;
    }
}

public class ExtensionTarget
{
}

public static class TargetExtensions
{
    public static void Ping(this ExtensionTarget t)
    {
    }
}

public class ExtensionUser
{
    public void Run()
    {
        var shouted = "x".Shout();
        var doubled = 3.Twice();
        var picked = new[] { 1 }.Pick();
        new ExtensionTarget().Ping();
        var target = new ExtensionTarget();
        target.Ping();
        StaticOnly.Reset();
        var alsoShouted = TextExtensions.Shout("y");
        var numbers = new[] { 1, 2, 3 };
        ref int first = ref numbers.RefFirst();
        first = 9;
        Console.WriteLine($"{shouted}{doubled}{picked}{StaticOnly.Value}{alsoShouted}{numbers[0]}");
    }
}
