// References: ref/in/out parameters, ref returns, ref fields, stackalloc, inline arrays, unsafe pointers, function pointers.
namespace Syntax.Memory;

ref struct RefBuffer
{
    public Span<int> Data;
    public ref int RefField;

    public int First => Data[0];
}

readonly ref struct RefView
{
    public readonly ReadOnlySpan<int> Data;

    public RefView(ReadOnlySpan<int> d)
    {
        Data = d;
    }
}

[System.Runtime.CompilerServices.InlineArray(4)]
struct InlineFour
{
    private int _element;
}

struct MemPoint
{
    public int X;
    public int Y;

    public void Touch()
    {
    }
}

class MemUser
{
    ref int Find(ref int x) => ref x;

    void In(in MemPoint p)
    {
    }

    void Out(out MemPoint p) => p = default;

    ref readonly int RefRo(ref readonly int x) => ref x;

    void Scoped(scoped ref MemPoint p)
    {
    }

    int Sum(params int[] xs) => xs.Length;

    int SumSpan(params ReadOnlySpan<int> span) => span.Length;

    static int Static(int x) => x;

    public void Run()
    {
        int local = 0;
        MemPoint pt = default;

        Find(ref local);
        In(in pt);
        Out(out var made);
        made.X = 1;
        Out(out MemPoint typed);
        typed.Y = 2;

        ref int r = ref Find(ref local);
        ref readonly int rr = ref RefRo(in local);

        Span<int> s = stackalloc int[4];
        var buf = new RefBuffer { Data = s };
        _ = buf.First;

        InlineFour four = default;
        four[0] = 1;

        dynamic d = new MemPoint();
        d.X = 1;
        d.Touch();

        Sum(1, 2);
        SumSpan(1, 2);

        nint ni = 1;
        nuint nu = 2;

        _ = new RefView(s).Data.Length;

        unsafe
        {
        }

        Scoped(ref pt);

        _ = r;
        _ = rr;
        _ = ni;
        _ = nu;
    }

    unsafe void Raw(int[] arr)
    {
        int local = 0;
        int* p = &local;
        *p = 1;

        fixed (int* q = arr)
        {
            q[0] = 1;
        }

        MemPoint pt = default;
        MemPoint* pp = &pt;
        pp->X = 1;
        pp->Touch();
        _ = sizeof(MemPoint);

        delegate*<int, int> fp = &Static;
        _ = fp(1);

        int* arr2 = stackalloc int[2];
        arr2[0] = 1;
    }
}
