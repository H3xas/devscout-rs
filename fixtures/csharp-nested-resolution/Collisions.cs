namespace First
{
    public class Owner
    {
        public class Shared { }
    }
}

namespace Second
{
    public class Owner
    {
        public class Shared { }
    }
}

namespace Unrelated
{
    public class Consumer
    {
        private Shared value;
    }
}
