// References: object creation, target-typed new, initializers, collection expressions.
namespace Syntax.Creation;

class CreatedItem
{
    public CreatedItem()
    {
    }

    public CreatedItem(int id)
    {
        Id = id;
    }

    public int Id { get; set; }
    public List<string> Tags { get; } = new();
}

class CreatedItem<T> : CreatedItem
{
}

class CreationUser
{
    public void Run()
    {
        _ = new CreatedItem();
        _ = new CreatedItem(1);
        CreatedItem b = new();
        _ = b.Tags;
        CreatedItem c = new(2);
        CreatedItem d = new() { Id = 1 };
        _ = new CreatedItem { Id = 2, Tags = { "a" } };
        _ = new List<CreatedItem> { new(), new CreatedItem() };
        _ = new Dictionary<int, CreatedItem> { [1] = new() };
        CreatedItem[] f = [new(), new()];
        int[] h = [1, 2, 3];
        int[] g = [1, ..h];
        var implicitArray = new[] { 1, 2 };
        var arrayOfThree = new CreatedItem[3];
        var jagged = new int[2][];
        var anon = new { Id = 1, Name = "x" };
        _ = new CreatedItem<int>();
        Take(new());
        bool flag = true;
        CreatedItem picked = flag ? new CreatedItem() : new();
        int firstId = new CreatedItem().Id;

        _ = b;
        _ = c;
        _ = d;
        _ = g;
        _ = implicitArray;
        _ = arrayOfThree;
        _ = jagged;
        _ = anon;
        _ = picked;
        _ = firstId;
    }

    void Take(CreatedItem x)
    {
        _ = x.Id;
    }
}
