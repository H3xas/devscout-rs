namespace BusRegistrationMorphology
{
    public static class AssemblyScanInstallation
    {
        public static void Install(IPressRegistry registry)
        {
            registry.AddEditionsFromAssembly(typeof(EditionHandler).Assembly);
        }
    }
}
