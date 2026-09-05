// Declarations: struct primary constructor, arithmetic/equality/comparison/conversion/true-false/checked/increment/logical-not operators, static-abstract operator via generic interface.
namespace Syntax.Operators;

public readonly struct OpMoney(decimal amount)
{
    public decimal Amount { get; } = amount;

    public static OpMoney operator +(OpMoney a, OpMoney b) => new(a.Amount + b.Amount);

    public static OpMoney operator -(OpMoney a) => new(-a.Amount);

    public static OpMoney operator -(OpMoney a, OpMoney b) => new(a.Amount - b.Amount);

    public static bool operator ==(OpMoney a, OpMoney b) => a.Amount == b.Amount;

    public static bool operator !=(OpMoney a, OpMoney b) => !(a == b);

    public static bool operator <(OpMoney a, OpMoney b) => a.Amount < b.Amount;

    public static bool operator >(OpMoney a, OpMoney b) => a.Amount > b.Amount;

    public static implicit operator decimal(OpMoney a) => a.Amount;

    public static explicit operator OpMoney(decimal d) => new(d);

    public static bool operator true(OpMoney a) => a.Amount != 0;

    public static bool operator false(OpMoney a) => a.Amount == 0;

    public static OpMoney operator checked +(OpMoney a, OpMoney b) => new(checked(a.Amount + b.Amount));

    public static OpMoney operator ++(OpMoney a) => new(a.Amount + 1);

    public static OpMoney operator !(OpMoney a) => new(-a.Amount);

    public override bool Equals(object? obj) => obj is OpMoney other && this == other;

    public override int GetHashCode() => Amount.GetHashCode();
}

public interface IOpAddable<TSelf> where TSelf : IOpAddable<TSelf>
{
    static abstract TSelf operator +(TSelf l, TSelf r);
}

public struct OpVector(int x) : IOpAddable<OpVector>
{
    public int X = x;

    public static OpVector operator +(OpVector l, OpVector r) => new(l.X + r.X);
}

public class OpUser
{
    public void Run()
    {
        var a = new OpMoney(1m);
        var b = new OpMoney(2m);
        var sum = a + b;
        var negated = -a;
        var diff = a - b;
        var equal = a == b;
        var notEqual = a != b;
        var less = a < b;
        var greater = a > b;
        decimal asDecimal = a;
        var fromDecimal = (OpMoney)3m;
        var checkedSum = checked(a + b);
        var incremented = a++;
        var not = !a;
        if (a)
        {
            Console.WriteLine("truthy");
        }

        var v1 = new OpVector(1);
        var v2 = new OpVector(2);
        var v3 = v1 + v2;
        Console.WriteLine($"{sum.Amount}{negated.Amount}{diff.Amount}{equal}{notEqual}{less}{greater}{asDecimal}{fromDecimal.Amount}{checkedSum.Amount}{incremented.Amount}{not.Amount}{v3.X}");
    }
}
