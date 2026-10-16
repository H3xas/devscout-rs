namespace Nautical.Crew
{
    public class Purser
    {
        public void Record(Nautical.Crew.Manifest manifest)
        {
            manifest.Seal();
        }
    }
}
