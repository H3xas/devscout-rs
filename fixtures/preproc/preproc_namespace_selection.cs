// A file whose namespace and `using` directives are chosen by a preprocessor
// symbol: the vendored-library convention of compiling one source file into
// two namespaces. With no symbols defined only the `#else` arm is compiled,
// so the graph must carry every type here exactly once, under
// `Fixtures.Preproc.Lattice`, and never under the `#if` arm's namespace.
// The `using` directive of the dead arm is not recorded either.
//
// Fully synthetic -- no identifiers below come from any real codebase.
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
