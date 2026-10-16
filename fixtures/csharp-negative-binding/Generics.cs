namespace NegativeBinding.Generics
{
    public class Router
    {
        public void Route<T>(T message) { }
        public void Route(object message) { }
    }

    public class Routing
    {
        public void Go()
        {
            var router = new Router();
            router.Route("payload");
        }
    }

    public abstract class Assembler
    {
        public void Attach<T>(T part) { }
    }

    public class RobotAssembler : Assembler
    {
        public void Attach(string part) { }
    }

    public class Assembling
    {
        public void Go()
        {
            var assembler = new RobotAssembler();
            assembler.Attach("arm");
        }
    }

    public class Pairer
    {
        public void Join<T>(T first, T second) { }
        public void Join<T1, T2>(T1 first, T2 second) { }
    }

    public class Pairing
    {
        public void Go()
        {
            var pairer = new Pairer();
            pairer.Join(1, "two");
        }
    }

    public class Sorter
    {
        public void Rank<T>(T item) where T : struct { }
        public void Rank(object item) { }
    }

    public class Sorting
    {
        public void Go()
        {
            var sorter = new Sorter();
            sorter.Rank(42);
        }
    }
}
