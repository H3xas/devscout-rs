using System;
using MassTransit;
using Fixture.Refusals.Fakes;

namespace Fixture.Refusals
{
    public class LookalikeType
    {
        private readonly IBusControl _control;

        public void Send()
        {
            _control.GetSendEndpoint(new Uri("queue:refused-lookalike-type"));
        }
    }
}
