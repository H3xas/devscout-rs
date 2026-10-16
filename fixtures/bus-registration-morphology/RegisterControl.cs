namespace BusRegistrationMorphology
{
    public static class ControlInstallation
    {
        public static void Install(IPressRegistry registry)
        {
            registry.AddEdition<EditionHandler>();
        }
    }
}
