namespace BusRegistrationMorphology
{
    public static class PluralRoleInstallation
    {
        public static void Install(IPressRegistry registry)
        {
            registry.Handlers(typeof(EditionHandler));
        }
    }
}
