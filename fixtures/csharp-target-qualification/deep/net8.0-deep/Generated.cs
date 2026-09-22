using TargetQualification.Deep.Generated;

namespace TargetQualification.Deep
{
    /// <summary>
    /// Consumes the source-generator's output: <c>GeneratedMarker</c> exists only as
    /// compile-time-generated input, never as a hand-authored file, and project loading must
    /// still discover it.
    /// </summary>
    public class GeneratedInputCaller
    {
        public string Ping()
        {
            return new GeneratedMarker().Origin();
        }
    }
}
