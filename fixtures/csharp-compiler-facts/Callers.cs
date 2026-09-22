namespace CompilerFacts.Widgets;

public class Callers
{
    public void Invoke()
    {
        var widget = new Widget();
        widget.Render(); widget.Render(true);

        var helper = new Helper();
        helper.Assist(); helper.Assist();
        helper.Secret();

        widget.Render("mismatched");

        dynamic dynamicArgument = true;
        widget.Render(dynamicArgument);
        helper.Choose(dynamicArgument);
    }

    public class Nested<T>
    {
        public void Go()
        {
            new Helper().Assist();
        }
    }
}
