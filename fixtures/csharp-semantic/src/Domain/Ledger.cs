using System;

namespace Fixture.Ext
{
    public static class LedgerExtensions
    {
        public static void Splice<T>(this Fixture.Domain.Ledger l, Func<T, object> make) { }
    }
}

namespace Fixture.Domain
{
    using Fixture.Ext;

    public class Note
    {
    }

    public class Chapter
    {
    }

    public class Ledger
    {
        public void Splice<TA, TB>(Func<TA, TB, object> make) { }

        public void UseOneParamLambda()
        {
            this.Splice((Note x) => new object());
        }

        public void UseOneParamLambdaWithTypeArgument()
        {
            this.Splice<Chapter>((Chapter x) => new object());
        }

        public void UseTwoParamLambda()
        {
            this.Splice((Note a, Chapter b) => new object());
        }

        public void UseOneParamLocalFunction()
        {
            object Make(Note x) => new object();
            this.Splice<Note>(Make);
        }

        public void UseTwoParamLocalFunction()
        {
            object Make(Note a, Chapter b) => new object();
            this.Splice<Note, Chapter>(Make);
        }
    }
}
