namespace Catalog
{
    public interface ICatalogue { }

    public interface ICatalogue<T>
    {
        void Shelve(T item);
    }
}
