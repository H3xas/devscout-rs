using System;
using System.Linq.Expressions;

namespace BusExtensionRefusal
{
    public static class ProbeHelpers
    {
        public static void VerifiedOnce<TTarget>(this TTarget source, Expression<Action<TTarget>> expression)
        {
        }

        public static void NeverCalled<TTarget>(this TTarget source, Expression<Action<TTarget>> expression)
        {
        }

        public static void RunsDirectly<TTarget>(this TTarget source, Action<TTarget> callback)
        {
            callback(source);
        }
    }
}
