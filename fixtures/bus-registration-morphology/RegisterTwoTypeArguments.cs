namespace BusRegistrationMorphology
{
    public static class TwoTypeArgumentInstallation
    {
        public static void Install(IPressRegistry registry)
        {
            registry.AddFolder<EditionHandler, EditionState>();
        }
    }
}
