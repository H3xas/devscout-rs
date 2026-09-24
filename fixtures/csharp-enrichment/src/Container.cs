namespace Fixtures.Enrichment
{
    public class Container<T>
    {
        private readonly T[] items;

        public Container(T[] items)
        {
            this.items = items;
        }

        public T this[int index] => items[index];
    }
}
