namespace BusSignals
{
    public class ArticleDraft
    {
        public string Title { get; set; }

        public void Publish()
        {
            Title = Title?.Trim();
        }
    }
}
