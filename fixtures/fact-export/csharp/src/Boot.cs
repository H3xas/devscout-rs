namespace Fixture.Use
{
    public class Boot
    {
        public int Start() => Config.Load();

        public int Total() => new Model.Ledger().Total() + new Model.Ledger().Count();
    }
}
