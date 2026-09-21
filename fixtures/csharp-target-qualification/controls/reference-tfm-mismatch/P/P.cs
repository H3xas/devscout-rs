namespace TargetQualification.Controls.ReferenceTfmMismatch
{
    /// <summary>
    /// P targets net8.0 and references Q, which targets net9.0 -- one TFM major version newer.
    /// The sidecar's own health check does not compare a reference's declared TFM against the
    /// referencing project's own TFM, so this mismatch passes today's restore and reports a
    /// healthy unit; the qualification row records that honestly as a defect, not a pass.
    /// </summary>
    public class PCaller
    {
        public string Call(QService service)
        {
            return service.Describe();
        }
    }
}
