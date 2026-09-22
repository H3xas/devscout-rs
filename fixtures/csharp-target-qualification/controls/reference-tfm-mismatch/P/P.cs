namespace TargetQualification.Controls.ReferenceTfmMismatch
{
    /// <summary>
    /// P targets net8.0 and references Q, which targets net9.0 -- one TFM major version newer.
    /// dotnet restore correctly rejects this mismatch with NU1201. The sidecar's own
    /// independent project-loading path does not compare a reference's declared TFM against the
    /// referencing project's own TFM, so it still reports P's unit as healthy; the
    /// qualification row records that divergence honestly as a defect, not a pass.
    /// </summary>
    public class PCaller
    {
        public string Call(QService service)
        {
            return service.Describe();
        }
    }
}
