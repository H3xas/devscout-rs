// Declarations: block namespace, dotted nested block namespace, using directive inside a namespace block.
namespace Syntax.Blocks
{
    public class BlockHost
    {
        public void Run()
        {
        }
    }
}

namespace Syntax.Blocks.Inner
{
    using Syntax.Blocks;

    public class InnerHost
    {
        public void Run() => new BlockHost().Run();
    }
}
