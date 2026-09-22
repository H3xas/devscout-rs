using Catalog;

namespace Consumers;

public class WideConsumer
{
    private ICatalogue<string, string> shelf;

    public void Stock(string item)
    {
        shelf.Shelve(item);
    }
}

public class NarrowConsumer
{
    private ICatalogue<string> shelf;

    public void Stock(string item)
    {
        shelf.Shelve(item);
    }
}
