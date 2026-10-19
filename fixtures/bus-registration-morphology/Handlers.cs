using System.Threading.Tasks;

namespace BusRegistrationMorphology
{
    public class EditionHandler : PressHandlerBase<EditionNotice>
    {
        public override Task Handle(EditionNotice notice) => Task.CompletedTask;
    }
}
