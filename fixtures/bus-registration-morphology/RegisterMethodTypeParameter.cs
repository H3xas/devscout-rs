namespace BusRegistrationMorphology
{
    public static class GenericInstallation
    {
        public static void Install<TShelf>(IPressRegistry registry)
        {
            registry.AddEdition<TShelf>();
        }
    }
}
