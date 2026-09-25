using System;

namespace BusVocabularyAdmission
{
    public class PilotRequest : IEquatable<PilotRequest>
    {
        public string Vessel { get; set; }

        public bool Equals(PilotRequest other) => other != null && Vessel == other.Vessel;

        public override bool Equals(object obj) => Equals(obj as PilotRequest);

        public override int GetHashCode() => Vessel?.GetHashCode() ?? 0;
    }

    public class BerthNotice
    {
        public string Berth { get; set; }
    }

    public class TerminalNotice
    {
        public string Terminal { get; set; }
    }

    public class TideNotice
    {
        public int HeightCm { get; set; }
    }

    public class EchoNotice
    {
        public string Sounding { get; set; }
    }

    public class SelfCarriedNotice : LedgerBase<BerthNotice>
    {
        public string Origin { get; set; }

        public override System.Threading.Tasks.Task Read(BerthNotice notice) =>
            System.Threading.Tasks.Task.CompletedTask;
    }
}
