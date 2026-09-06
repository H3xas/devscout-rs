using Widgetworks.Catalog;
using Xunit;

namespace Widgetworks.Catalog.Tests;

public class CatalogStoreTests
{
    [Fact]
    public void CountItems_StartsAtZero()
    {
        var store = new InMemoryCatalogStore();
        Assert.Equal(0, store.CountItems());
    }
}
