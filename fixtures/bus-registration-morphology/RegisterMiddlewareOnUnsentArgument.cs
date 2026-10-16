namespace BusRegistrationMorphology
{
    public static class UnsentArgumentMiddleware
    {
        public static void Install(IPressRegistry registry)
        {
            registry.UseMiddleware<LedgerBridge>();
        }
    }
}
