namespace BusRegistrationMorphology
{
    public static class AssemblyTypeRegistration
    {
        public static void Install(IPressRegistry registry)
        {
            registry.RegisterEditionTypes(typeof(EditionHandler).Assembly);
        }
    }
}
