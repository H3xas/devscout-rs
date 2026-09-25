namespace BusVocabulary
{
    public class ReturnsDeskNotice
    {
        public string Aisle { get; set; }
    }

    public class OverdueNotice
    {
        public int DaysLate { get; set; }
    }

    public class ShelfAuditNotice
    {
        public string Shelf { get; set; }
    }

    public class UnregisteredNotice
    {
        public string Reason { get; set; }
    }
}
