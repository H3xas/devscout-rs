namespace CompilerFacts.Widgets;

public class Widget
{
    public void Render() { } public void Render(bool flag) { }

    public void Load()
    {
        this.MissingBinding();
    }

    public void Reference()
    {
        System.Console.WriteLine(UndeclaredSite);
    }
}

public class Gadget
{
    public void Ping()
    {
    }
}
