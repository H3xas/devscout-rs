namespace BusRegistrationMorphology
{
    public static class ReceivedArgumentMiddleware
    {
        public static void Install(IPressRegistry registry)
        {
            registry.UseMiddleware<EagerBridge>();
        }
    }
}
