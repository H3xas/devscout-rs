using System.Collections.Generic;

namespace Widgetworks.Catalog;

public class InMemoryCatalogStore : ICatalogStore
{
    private readonly List<string> _items = new();

    public int CountItems()
    {
        return _items.Count;
    }

    public void AddItem(string name)
    {
        _items.Add(name);
    }
}
