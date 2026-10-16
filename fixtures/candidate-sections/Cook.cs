namespace Nautical.Crew
{
    public class Cook
    {
        public void Boil(Galley galley, Fleet.Nautical.Crew.Galley reliefGalley)
        {
            galley.Simmer(2);
            reliefGalley.Simmer(4);
        }
    }
}
