#define TRACE_ON
#undef NEVER_ON
// Preprocessor: #define/#undef, #if/#elif/#else, #region, #nullable, #pragma warning, #line, #warning.
namespace Syntax.Preproc;

#if TRACE_ON
using System.Text;
#endif

class PreprocMarker
{
    public int Compound;
    public int Nested;
    public int Dead;
}

class PreprocHost
{
    private static PreprocMarker Marker => new();
    public void DebugPath()
    {
    }

    public void TracePath()
    {
    }

    public void ReleasePath()
    {
    }

    public void LivePath()
    {
    }

    public void NestedPath()
    {
    }

    public void Run()
    {
#if DEBUG
        DebugPath();
#elif TRACE_ON
        TracePath();
#else
        ReleasePath();
#endif

#if !NEVER_ON
        LivePath();
#endif

#if TRACE_ON && !NEVER_ON
        TracePath();
        Marker.Compound = 1;
#endif

#if TRACE_ON
#if !NEVER_ON
        NestedPath();
        Marker.Nested = 1;
#endif
#endif

#if NEVER_ON
        Marker.Dead = 1;
#endif
    }

#if NEVER_ON
    public void DeadMethod()
    {
        PreprocDeadType.Touch();
    }
#endif

    #region Helpers
    void Helper()
    {
    }
    #endregion Helpers

#nullable disable
    public string DisabledField;
#nullable restore

    void UnusedLocalDemo()
    {
#pragma warning disable CS0168
        int unused;
#pragma warning restore CS0168
    }

#line 200 "Generated.cs"
    void Generated()
    {
    }
#line default

#line hidden
    void Hidden()
    {
    }
#line default
}

#if NEVER_ON
class PreprocDeadType
{
    public static void Touch()
    {
    }
}
#endif

#warning fixture warning
