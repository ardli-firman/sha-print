using System;
using System.Threading;
using System.Threading.Tasks;

namespace ShaPrint.WpfApp.Services.Server
{
    /// <summary>
    /// Indirection over Task.Delay so the monitor's poll interval can be
    /// observed by unit tests without wall-clock waits. The real
    /// implementation just calls Task.Delay.
    /// </summary>
    public interface IDelayProbe
    {
        Task Delay(TimeSpan delay, CancellationToken cancellationToken);
    }

    public sealed class SystemDelayProbe : IDelayProbe
    {
        public Task Delay(TimeSpan delay, CancellationToken cancellationToken)
            => Task.Delay(delay, cancellationToken);
    }
}
