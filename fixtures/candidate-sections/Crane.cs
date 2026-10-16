namespace Nautical.Crew
{
    public class Crane
    {
        public void Winch(IHold hold)
        {
            hold.Stow(3);
        }
    }
}
