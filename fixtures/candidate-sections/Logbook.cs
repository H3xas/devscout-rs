namespace Nautical.Crew
{
    public class Logbook
    {
        [Fact]
        public void Records(Harbor.Office.Manifest manifest)
        {
            manifest.Seal();
        }
    }
}
