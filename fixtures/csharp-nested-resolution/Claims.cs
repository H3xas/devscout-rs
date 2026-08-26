namespace LocalTests
{
    public class Scenario
    {
        public class Claim { }
    }
}

namespace BclConsumer
{
    using System.Security.Claims;

    public class Handler
    {
        private Claim claim;
    }
}
