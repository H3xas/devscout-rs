#if LIGHT_LATTICE
using static Fixtures.Preproc.LightLattice.Expression;
namespace Fixtures.Preproc.LightLattice
#else
using static Fixtures.Preproc.Lattice.Expression;
namespace Fixtures.Preproc.Lattice
#endif
{
    public static partial class LatticeCompiler
    {
        public static Closure Compile(Expression tree)
        {
            var closure = new Closure();
            LatticeEmitter.Emit(tree, closure);
            return closure;
        }

        private enum EmitMode : byte
        {
            Direct = 0,
            Delegated = 1,
        }
    }

    internal static class LatticeEmitter
    {
        public static void Emit(Expression tree, Closure closure)
        {
            closure.Depth = tree.Depth;
        }
    }

    public sealed class Closure
    {
        public int Depth { get; set; }
    }

    public abstract class Expression
    {
        public abstract int Depth { get; }
    }
}
