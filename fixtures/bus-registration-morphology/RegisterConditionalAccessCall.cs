namespace BusRegistrationMorphology
{
    public static class ConditionalAccessInstallation
    {
        public static void Install(IPressRegistry registry)
        {
            registry?.AddEdition<EditionHandler>();
        }
    }
}
