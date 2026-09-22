namespace Catalog
{
    public interface ICatalogue<T>
    {
        void Shelve(T item);
    }

    public interface ICatalogue { }
}
