namespace TargetQualification.Deep.IncompatibleRef
{
    /// <summary>
    /// This library targets net472 only. A modern consumer restores it under an
    /// asset-compatibility fallback rather than a native match, so a healthy-looking restore
    /// can still hide an incompatible reference.
    /// </summary>
    public class LegacyOnly
    {
        public string Describe()
        {
            return "legacy-only";
        }
    }
}
