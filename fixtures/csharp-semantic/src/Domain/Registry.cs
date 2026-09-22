namespace Fixture.Domain;

public interface IDictionary<T>
{
    bool ContainsKey(T key);
}

public interface IDictionary
{
}

public class WideRegistryConsumer
{
    private IDictionary<string, string> _table = null!;

    public bool Check(string key) => _table.ContainsKey(key);
}

public class NarrowRegistryConsumer
{
    private IDictionary<string> _table = null!;

    public bool Check(string key) => _table.ContainsKey(key);
}
