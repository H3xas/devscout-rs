namespace TargetQualification.Deep.IncompatibleRefModern
{
    /// <summary>
    /// This library targets net8.0 only. A .NET Framework consumer cannot restore it at all --
    /// the opposite direction of the legacy-only incompatible reference, and the sharper of the
    /// two failure shapes a project system must surface honestly.
    /// </summary>
    public class ModernOnly
    {
        public string Describe()
        {
            return "modern-only";
        }
    }
}
