using System;
using System.Linq.Expressions;

namespace BusOptionalAlignment
{
    public static class CheckHelpers
    {
        public static void Confirmed<TTarget>(
            this TTarget source,
            Expression<Action<TTarget>> expression,
            string? note = null)
        {
        }

        public static void ConfirmedStatic<TTarget>(
            TTarget target,
            Expression<Action<TTarget>> expression,
            string? note = null)
        {
        }

        public static void RunsLive<TTarget>(
            this TTarget source,
            Action<TTarget> callback,
            string? note = null)
        {
            callback(source);
        }
    }
}
