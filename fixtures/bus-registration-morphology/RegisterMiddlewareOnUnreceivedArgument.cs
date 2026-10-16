namespace BusRegistrationMorphology
{
    public static class UnreceivedArgumentMiddleware
    {
        public static void Install(IPressRegistry registry)
        {
            registry.UseMiddleware<QuietBridge>();
        }
    }
}
