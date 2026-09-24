using System;
using System.Linq.Expressions;

namespace BusSignals
{
    // A repository-declared helper wrapping a test double: `ExpectPublish`
    // takes its lambda as an `Expression<Action<IBus>>` -- an expression
    // TREE, the shape that turns a lambda into data a mocking library
    // inspects rather than a delegate it invokes -- so the `PublishAsync`
    // call inside never actually runs. Must earn no hop, the repository-
    // helper half of the same refusal `TestDoubleSetupLambda.cs`/
    // `TestDoubleVerifyLambda.cs` cover directly against the library.
    public static class LoanRequestedTestHelpers
    {
        public static void ExpectPublish(Expression<Action<IBus>> setup)
        {
        }
    }

    public class LoanRequestedHelperExpectation
    {
        public void Arrange()
        {
            LoanRequestedTestHelpers.ExpectPublish(x => x.PublishAsync<LoanRequested>(default));
        }
    }
}
