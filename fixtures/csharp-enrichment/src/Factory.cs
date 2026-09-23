namespace Fixtures.Enrichment
{
    public class Factory
    {
        public T Get<T>() where T : new() => new T();
    }
}
