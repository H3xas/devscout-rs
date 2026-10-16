namespace BusRegistrationMorphology
{
    public static class WithNameInstallation
    {
        public static void Install(IPressRegistry registry)
        {
            registry.With<EditionHandler>();
        }
    }
}
