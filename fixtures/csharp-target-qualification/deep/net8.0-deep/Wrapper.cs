using TargetQualification.Shared;

namespace TargetQualification.Deep.Wrapper
{
    /// <summary>
    /// A configured wrapper: implements the shared <see cref="IContract"/> by delegating to an
    /// inner instance rather than declaring its own logic, the shape a dependency-injection
    /// registration commonly wraps around a concrete implementation.
    /// </summary>
    public class LoggingContractWrapper : IContract
    {
        private readonly IContract _inner;

        public LoggingContractWrapper(IContract inner)
        {
            _inner = inner;
        }

        public string Describe()
        {
            return _inner.Describe();
        }
    }

    /// <summary>
    /// Calls through the wrapper's own declared type, not through the shared <see
    /// cref="IContract"/> the positive case already exercises, so the wrapper family has its
    /// own reference edge instead of borrowing the positive case's.
    /// </summary>
    public class WrapperCaller
    {
        public string CallWrapper(LoggingContractWrapper wrapper)
        {
            return wrapper.Describe();
        }
    }
}
