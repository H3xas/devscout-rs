// Declarations: fields (instance/static/readonly/const/volatile), properties (auto/backed/get-only/init/required/expression-bodied/private-set/field-keyword), indexers, events (field-like/explicit), ctors (instance/chained/static), finalizer, overloads, static/async/expression-bodied methods, virtual/abstract/override/new, protected internal, ref readonly return.
namespace Syntax.Members;

public abstract class MemberBase
{
    public abstract string Describe();

    public virtual string Hide() => "base";
}

public interface IMemberDependency
{
    int Seed { get; }
}

public class MemberDependency : IMemberDependency
{
    public int Seed => 1;
}

public class MemberHost : MemberBase
{
    public int InstanceField;
    public static int StaticField;
    public readonly int ReadonlyField;
    public const int Limit = 3;
    public volatile int VolatileField;
    public int Left, Right;

    public int AutoProperty { get; set; }

    private int _backing;
    public int BodiedProperty
    {
        get => _backing;
        set => _backing = value;
    }

    public int GetOnlyProperty { get; }

    public int InitProperty { get; init; }

    public required int RequiredProperty { get; set; }

    public int ExpressionProperty => InstanceField + 1;

    public int PrivateSetProperty { get; private set; }

    public int Age { get; set => field = value < 0 ? 0 : value; }

    public int this[int i] => i;

    public int this[int a, int b] => a + b;

    public event EventHandler? Changed;

    private EventHandler? _clicked;
    public event EventHandler? Clicked
    {
        add { _clicked += value; }
        remove { _clicked -= value; }
    }

    public MemberHost()
        : this(0)
    {
    }

    public MemberHost(int readonlyValue)
    {
        ReadonlyField = readonlyValue;
    }

    public MemberHost(IMemberDependency dependency)
        : this(dependency.Seed)
    {
    }

    public IMemberDependency Dependency() => new MemberDependency();

    static MemberHost()
    {
        StaticField = 1;
    }

    ~MemberHost()
    {
    }

    public int Add(int x) => x;

    public int Add(int x, int y) => x + y;

    public static int StaticMethod() => 1;

    public async Task AsyncMethod()
    {
        await Task.Yield();
    }

    public int ExpressionMethod() => 2;

    public override string Describe() => "host";

    public new string Hide() => "hidden";

    protected internal int ProtectedInternalMethod() => 3;

    private int _refValue = 5;
    public ref readonly int RefReadonlyMethod() => ref _refValue;

    public void RaiseChanged()
    {
        Changed?.Invoke(this, EventArgs.Empty);
        _clicked?.Invoke(this, EventArgs.Empty);
    }

    public void SetPrivateSetProperty(int value)
    {
        PrivateSetProperty = value;
    }
}

public class MemberUser
{
    public async Task Run()
    {
        var host = new MemberHost { RequiredProperty = 5, InitProperty = 7 };
        host.InstanceField = 1;
        MemberHost.StaticField = 2;
        host.VolatileField = 1;
        host.AutoProperty = 1;
        host.BodiedProperty = 2;
        host.SetPrivateSetProperty(9);
        host.Age = -5;
        host.Changed += (_, _) => { };
        host.Clicked += (_, _) => { };
        host.RaiseChanged();
        var indexed = host[0] + host[1, 2];
        var added = host.Add(1) + host.Add(1, 2);
        var staticValue = MemberHost.StaticMethod();
        await host.AsyncMethod();
        var expr = host.ExpressionMethod();
        var described = host.Describe();
        var hidden = host.Hide();
        var protectedInternal = host.ProtectedInternalMethod();
        ref readonly int refVal = ref host.RefReadonlyMethod();
        Console.WriteLine($"{host.ReadonlyField}{MemberHost.Limit}{host.GetOnlyProperty}{host.RequiredProperty}{host.InitProperty}{host.ExpressionProperty}{host.PrivateSetProperty}{host.Age}{indexed}{added}{staticValue}{expr}{described}{hidden}{protectedInternal}{refVal}");
    }
}
