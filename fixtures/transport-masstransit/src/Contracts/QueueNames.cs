namespace Fixture.Contracts
{
    public static class QueueNames
    {
        public const string Prefix = "ledger-";
        public const string Audit = Prefix + "audit";
        public const string Loop = Echo;
        public const string Echo = Loop;
        public static readonly string Rotating = "rotating";
        public static string Mutable = "mutable";
    }
}
