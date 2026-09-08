using Fixture.Ext.Contracts;

namespace Fixture.Domain;

public class Binder
{
    public void Dispose() { }
}

public class BinderHolder
{
    public IBinder Binder { get; } = null!;

    public void Close() => Binder?.Dispose();
}
