namespace TargetQualification.Deep.UnknownFramework
{
    /// <summary>
    /// Carries a name and a method-name shape that read as a web-framework lifecycle hook by
    /// convention alone: no base type, no interface and no attribute ties it to any known
    /// framework. Name/suffix evidence must never be enough on its own to claim a framework
    /// adapter binding -- this type stays a naming coincidence, not evidence.
    /// </summary>
    public class WidgetController
    {
        public void OnActionExecuting()
        {
        }
    }
}
