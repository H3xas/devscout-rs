// Declarations: local functions (instance-capturing, static, generic, params, async, recursive, forward-declared, nested) and a lambda capturing a local function.
namespace Syntax.Locals;

public class LocalItem
{
    public int Value;

    public int Score() => Value;
}

public class LocalHost
{
    public async Task Run()
    {
        var item = new LocalItem();

        int Square(int x) => x * x;

        int Read(LocalItem local)
        {
            var scored = local.Score();
            return scored + item.Value;
        }

        Func<LocalItem, int> scoreAndRead = candidate => candidate.Score() + Read(candidate);
        _ = scoreAndRead(item);

        static int Cube(int x) => x * x * x;

        T Echo<T>(T v) => v;

        int Sum(params int[] values)
        {
            var total = 0;
            foreach (var v in values)
            {
                total += v;
            }

            return total;
        }

        async Task<int> DelayedDouble(int x)
        {
            await Task.Yield();
            return x * 2;
        }

        int Factorial(int n) => n <= 1 ? 1 : n * Factorial(n - 1);

        var forwardResult = UsedBeforeDeclaration(2);

        int UsedBeforeDeclaration(int x)
        {
            int Nested(int y) => y + 1;
            return Nested(x);
        }

        Func<int, int> f = x => Square(x);

        var squared = Square(3);
        var cubed = Cube(3);
        var echoed = Echo("value");
        var summed = Sum(1, 2, 3);
        var doubled = await DelayedDouble(4);
        var factorial = Factorial(4);
        var viaLambda = f(5);

        Console.WriteLine($"{squared}{cubed}{echoed}{summed}{doubled}{factorial}{forwardResult}{viaLambda}");
    }
}
