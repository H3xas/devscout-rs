namespace Nautical.Crew
{
    public class Quartermaster
    {
        public void Provision()
        {
            Galley.Stow(4);
            Larder.Stow(2);
            Galley.Ladle();
        }
    }
}
