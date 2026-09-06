namespace Widgetworks.Catalog;

public class CatalogController
{
    private readonly InMemoryCatalogStore _store = new InMemoryCatalogStore();

    public int ItemCount()
    {
        return _store.CountItems();
    }
}
