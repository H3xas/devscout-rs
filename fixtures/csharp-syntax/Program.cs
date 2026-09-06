// Declarations: top-level statements, top-level local function, block namespace declared after top-level statements.
using Syntax.TopLevel;

var host = new TopLevelHost();
host.Run();
Console.WriteLine(TopLevelHelper.Answer());

static int Local() => 1;
Local();

return 0;

namespace Syntax.TopLevel
{
    public class TopLevelHost
    {
        public void Run()
        {
        }
    }

    public static class TopLevelHelper
    {
        public static int Answer() => 42;
    }
}
