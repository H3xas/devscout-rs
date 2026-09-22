namespace TargetQualification.Shared
{
    public interface IContract
    {
        string Describe();
    }

    public class Service : IContract
    {
        public string Describe() => "service";
    }

    public class Caller
    {
        public string Invoke(IContract contract) => contract.Describe();
    }
}
