namespace BusRegistrationMorphology
{
    public static class LooseTargetInstallation
    {
        public static void Install(IPressRegistry registry)
        {
            registry.AddEdition<LooseHandler>();
        }
    }
}
