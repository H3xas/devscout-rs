// References: member access, conditional access, invocation, generic invocation, this/base, global::.
namespace Syntax.Access;

class AccessTarget
{
    public int Count;
    public string? Name { get; set; }
    public AccessTarget? Next;
    public int Compute() => Count;
    public T Generic<T>() => default!;
    public static AccessTarget Shared = new();
    public int this[int i] => i;
    public void Hook2() { }
    public AccessTarget Self() => this;
}

class AccessBase
{
    public int BaseField;
    public virtual void Hook() { }
}

class AccessUser : AccessBase
{
    private readonly AccessTarget target = new();
    private AccessTarget TargetProp { get; set; } = new();
    private static AccessTarget SharedProp { get; set; } = new();

    public async Task Run()
    {
        object o = target;

        _ = target.Count;
        target.Name = "x";
        target.Compute();
        target.Generic<int>();
        _ = target?.Compute();
        _ = target?.Next?.Name;
        _ = target.Next!.Count;
        _ = target[0];
        this.Run2();
        this.BaseField = 1;
        base.Hook();
        _ = global::Syntax.Access.AccessTarget.Shared.Compute();
        _ = AccessTarget.Shared.Next?.Compute();
        _ = target.Next.Next.Compute();
        Action a = target.Hook2;
        a();
        _ = new AccessTarget().Compute();
        _ = ((AccessTarget)o).Compute();
        _ = (await Load()).Compute();

        var v = new AccessTarget();
        _ = v.Compute();
        AccessTarget ex = new AccessTarget();
        _ = ex.Compute();

        UseParam(target);
        _ = TargetProp.Compute();
        _ = SharedProp.Compute();
        _ = target.Next.Compute();
        _ = target?[0];
        _ = target.Self().Compute();
    }

    private void Run2() { }

    private void UseParam(AccessTarget p) => p.Compute();

    public Task<AccessTarget> Load() => Task.FromResult(target);
}
