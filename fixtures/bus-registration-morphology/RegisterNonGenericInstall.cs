namespace BusRegistrationMorphology
{
    public static class NonGenericInstallation
    {
        public static void Install(IPressRegistry registry)
        {
            registry.AddEdition(typeof(EditionHandler));
        }
    }
}
