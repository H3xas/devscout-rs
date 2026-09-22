using System.Text.Json.Serialization;

namespace CsharpContext.Clean;

public sealed class Greeting
{
    public string Text { get; set; } = "";
}

// A real source generator (ships in the net9.0 shared framework, no new
// package) whose output this fixture's generated-document accounting names.
[JsonSerializable(typeof(Greeting))]
internal sealed partial class GreetingJsonContext : JsonSerializerContext
{
}

public static class Program
{
    public static void Main()
    {
        var greeting = new Greeting { Text = "hello" };
        System.Console.WriteLine(System.Text.Json.JsonSerializer.Serialize(greeting, GreetingJsonContext.Default.Greeting));
    }
}
