// Declarations: class, struct, readonly struct, ref struct, record, record class, record struct, readonly record struct, interface, enum, flags enum, delegate, abstract/sealed class, static class, file class.
namespace Syntax.Kinds;

public class KindClass : IKindInterface
{
    public void Touch()
    {
    }
}

public struct KindStruct
{
}

public readonly struct KindReadonlyStruct
{
}

public ref struct KindRefStruct
{
    public int Value;
}

public record KindRecord(int Id);

public record class KindRecordClass
{
}

public record struct KindRecordStruct(int Id);

public readonly record struct KindReadonlyRecordStruct(int Id);

public interface IKindInterface
{
    void Touch();
}

public enum KindEnum
{
    Alpha,
    Beta = 5,
}

[Flags]
public enum KindFlags : byte
{
    None = 0,
    First = 1,
    Second = 2,
}

public delegate int KindDelegate(int x);

public abstract class KindAbstract
{
}

public sealed class KindSealed : KindAbstract
{
}

public static class KindStatic
{
    public static int Value = 1;
}

file class KindFileLocal
{
}

public class KindUser
{
    public void Run()
    {
        var a = new KindClass();
        IKindInterface hook = a;
        hook.Touch();
        var s = new KindStruct();
        var ro = new KindReadonlyStruct();
        var refS = new KindRefStruct { Value = 7 };
        int refValue = refS.Value;
        var rec = new KindRecord(1);
        var recClass = new KindRecordClass();
        var recStruct = new KindRecordStruct(1);
        var readonlyRecStruct = new KindReadonlyRecordStruct(1);
        var enumValue = KindEnum.Beta;
        var flags = KindFlags.First | KindFlags.Second;
        KindDelegate del = x => x + 1;
        var sealedInstance = new KindSealed();
        var staticValue = KindStatic.Value;
        var fileLocal = new KindFileLocal();
        Console.WriteLine($"{a}{s}{ro}{refValue}{rec}{recClass}{recStruct}{readonlyRecStruct}{enumValue}{flags}{del(1)}{sealedInstance}{staticValue}{fileLocal}");
    }
}
