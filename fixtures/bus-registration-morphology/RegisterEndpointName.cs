namespace BusRegistrationMorphology
{
    public static class EndpointNameInstallation
    {
        public static void Install(IPressRegistry registry)
        {
            registry.Endpoint<EditionHandler>();
        }
    }
}
