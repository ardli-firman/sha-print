using System;
using System.Printing;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.WpfApp.Services.Server;
using Xunit;

namespace ShaPrint.Tests
{
    [Collection("Sequential")]
    public class PrintMonitorServiceTests
    {
        private static PrintMonitorService NewService()
        {
            return new PrintMonitorService(
                snackbarService: null!,
                notificationService: null!,
                probe: new NoopProbe(),
                delay: new ImmediateDelayProbe());
        }

        [Theory]
        [InlineData(PrintJobStatus.Error, true)]
        [InlineData(PrintJobStatus.PaperOut, true)]
        [InlineData(PrintJobStatus.Blocked, true)]
        [InlineData(PrintJobStatus.Error | PrintJobStatus.Printing, true)]
        [InlineData(PrintJobStatus.PaperOut | PrintJobStatus.Retained, true)]
        public void IsHardError_ShouldReturnTrue_ForErrorStatuses(PrintJobStatus status, bool expected)
        {
            Assert.Equal(expected, PrintMonitorService.IsHardError(status));
        }

        [Theory]
        [InlineData(PrintJobStatus.None, false)]
        [InlineData(PrintJobStatus.Printing, false)]
        [InlineData(PrintJobStatus.Spooling, false)]
        [InlineData(PrintJobStatus.Retained, false)]
        [InlineData(PrintJobStatus.Printed, false)]
        [InlineData(PrintJobStatus.Deleted, false)]
        [InlineData(PrintJobStatus.Offline, false)]
        [InlineData(PrintJobStatus.Paused, false)]
        [InlineData(PrintJobStatus.UserIntervention, false)]
        public void IsHardError_ShouldReturnFalse_ForNormalOrSoftStatuses(PrintJobStatus status, bool expected)
        {
            Assert.Equal(expected, PrintMonitorService.IsHardError(status));
        }

        [Fact]
        public void SetMonitoredPrinters_ShouldUpdateInternalList()
        {
            var service = NewService();
            service.SetMonitoredPrinters(new System.Collections.Generic.List<string> { "PrinterA", "PrinterB" });

            var field = typeof(PrintMonitorService).GetField("_monitoredPrinters",
                System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance);
            var monitored = field!.GetValue(service) as System.Collections.Generic.List<string>;

            Assert.NotNull(monitored);
            Assert.Equal(2, monitored!.Count);
            Assert.Contains("PrinterA", monitored);
            Assert.Contains("PrinterB", monitored);
        }

        [Fact]
        public void SetMonitoredPrinters_WithNull_ShouldInitializeEmptyList()
        {
            var service = NewService();
            service.SetMonitoredPrinters(null!);

            var field = typeof(PrintMonitorService).GetField("_monitoredPrinters",
                System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance);
            var monitored = field!.GetValue(service) as System.Collections.Generic.List<string>;

            Assert.NotNull(monitored);
            Assert.Empty(monitored!);
        }

        // Test helpers used by the updated ctor and the new tests.
        private sealed class NoopProbe : IPrintQueueProbe
        {
            public Task<System.Collections.Generic.IReadOnlyList<JobSnapshot>> GetJobsAsync(
                System.Collections.Generic.IEnumerable<string> monitoredPrinters, CancellationToken cancellationToken)
                => Task.FromResult<System.Collections.Generic.IReadOnlyList<JobSnapshot>>(
                    new System.Collections.Generic.List<JobSnapshot>());

            public Task CancelAsync(int jobId, string printerName, CancellationToken cancellationToken) => Task.CompletedTask;
        }

        private sealed class ImmediateDelayProbe : IDelayProbe
        {
            public Task Delay(System.TimeSpan delay, CancellationToken cancellationToken)
                => Task.CompletedTask;
        }
    }

    /// <summary>
    /// End-to-end behavior of PrintMonitorService's monitor loop: dedup,
    /// stable-error detection, eviction, streak cap, AutoPurge toggle,
    /// and poll interval.
    /// </summary>
    [Collection("Sequential")]
    public class PrintMonitorServiceBehaviorTests
    {
        private const string Printer = "TestPrinter";

        [Fact]
        public async Task HardError_AlertedOnce_ThenSuppressed()
        {
            var probe = new ScriptedProbe();
            probe.Queue(Job(1, Error));   // poll 1
            probe.Queue(Job(1, Error));   // poll 2 -> triggers
            probe.Queue(Job(1, Error));   // poll 3 -> suppressed
            var (svc, snack, toast, delay) = BuildService(probe);

            await RunPollsAsync(svc, snack, toast, 3);

            Assert.Equal(1, probe.CancelCallCount);
            Assert.Equal(1, snack.Shown);
            Assert.Equal(1, toast.Shown);
        }

        [Fact]
        public async Task SoftError_NeverAlerted()
        {
            var probe = new ScriptedProbe();
            probe.Queue(Job(1, Offline));
            probe.Queue(Job(1, Paused));
            probe.Queue(Job(1, UserIntervention));
            probe.Queue(Job(1, Offline));
            var (svc, snack, toast, delay) = BuildService(probe);

            await RunPollsAsync(svc, snack, toast, 4);

            Assert.Equal(0, probe.CancelCallCount);
            Assert.Equal(0, snack.Shown);
            Assert.Equal(0, toast.Shown);
        }

        [Fact]
        public async Task HardErrorRecoverBeforeSecondPoll_NotAlerted()
        {
            var probe = new ScriptedProbe();
            probe.Queue(Job(1, Error));       // poll 1: streak=1
            probe.Queue(Job(1, Printing));    // poll 2: streak reset
            probe.Queue(Job(1, Error));       // poll 3: streak=1 again
            probe.Queue(Job(1, Printing));    // poll 4: streak reset
            var (svc, snack, toast, delay) = BuildService(probe);

            await RunPollsAsync(svc, snack, toast, 4);

            Assert.Equal(0, probe.CancelCallCount);
            Assert.Equal(0, snack.Shown);
            Assert.Equal(0, toast.Shown);
        }

        [Fact]
        public async Task EvictOnJobDisappearance_AllowsReAlertOnNewJob()
        {
            var probe = new ScriptedProbe();
            probe.Queue(Job(1, Error));           // poll 1
            probe.Queue(Job(1, Error));           // poll 2 -> triggers, alert
            probe.Queue(/* empty */);             // poll 3 -> job 1 evicted
            probe.Queue(Job(2, Error));           // poll 4: new job, streak=1
            probe.Queue(Job(2, Error));           // poll 5: streak=2, alert
            var (svc, snack, toast, delay) = BuildService(probe);

            await RunPollsAsync(svc, snack, toast, 5);

            Assert.Equal(2, probe.CancelCallCount);
            Assert.Equal(2, snack.Shown);
            Assert.Equal(2, toast.Shown);
        }

        [Fact]
        public async Task EvictOnCancelThrow_AllowsReAlertNextPoll()
        {
            var probe = new ScriptedProbe();
            probe.Queue(Job(1, Error));                    // poll 1
            probe.Queue(Job(1, Error), throwOnCancel: true); // poll 2: cancel throws, evict
            probe.Queue(Job(1, Error));                    // poll 3: streak=1
            probe.Queue(Job(1, Error));                    // poll 4: streak=2, re-alert
            var (svc, snack, toast, delay) = BuildService(probe);

            await RunPollsAsync(svc, snack, toast, 4);

            Assert.Equal(2, probe.CancelCallCount);
            Assert.Equal(1, snack.Shown);
            Assert.Equal(1, toast.Shown);
        }

        [Fact]
        public async Task StreakCapAllowsReAlertAfterTenPolls()
        {
            var probe = new ScriptedProbe();
            for (int i = 0; i < 13; i++) probe.Queue(Job(1, Error));
            var (svc, snack, toast, delay) = BuildService(probe);

            await RunPollsAsync(svc, snack, toast, 13);

            // First alert at poll 2 (streak=2), cap drops at poll 11 (streak>10),
            // re-alert at poll 13 (streak=2 again).
            Assert.Equal(2, probe.CancelCallCount);
            Assert.Equal(2, snack.Shown);
            Assert.Equal(2, toast.Shown);
        }

        [Fact]
        public void PollInterval_IsTenSeconds()
        {
            var cts = new CancellationTokenSource();
            var delay = new CountingDelayProbe(cts);
            var (svc, snack, toast, _) = BuildService(new ScriptedProbe(), delay);
            var monitor = (Task)typeof(PrintMonitorService)
                .GetMethod("MonitorLoopAsync", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance)!
                .Invoke(svc, new object[] { cts.Token })!;

            // Wait for one delay call (loop runs once, then awaits delay).
            SpinWait.SpinUntil(() => delay.Requests.Count >= 1, 2000);
            svc.Stop();

            Assert.NotEmpty(delay.Requests);
            Assert.Equal(System.TimeSpan.FromSeconds(10), delay.Requests[0]);
        }

        [Fact]
        public async Task AutoPurgeDisabled_LoopSkips()
        {
            var probe = new ScriptedProbe();
            probe.Queue(Job(1, Error));
            probe.Queue(Job(1, Error));
            var (svc, snack, toast, delay) = BuildService(probe);

            var prior = ShaPrint.WpfApp.Models.AppSettings.Current.AutoPurgeEnabled;
            try
            {
                ShaPrint.WpfApp.Models.AppSettings.Current.AutoPurgeEnabled = false;
                await RunPollsAsync(svc, snack, toast, 2);
            }
            finally
            {
                ShaPrint.WpfApp.Models.AppSettings.Current.AutoPurgeEnabled = prior;
            }

            Assert.Equal(0, probe.CancelCallCount);
            Assert.Equal(0, snack.Shown);
        }

        // ─── helpers ───────────────────────────────────────────────────

        private static JobSnapshot Job(int id, PrintJobStatus status)
            => new(id, Printer, $"doc-{id}", status);

        private static (PrintMonitorService, CountingSnackbar, CountingNotification, CountingDelayProbe)
            BuildService(ScriptedProbe probe, CountingDelayProbe? delay = null)
        {
            ShaPrint.WpfApp.Models.AppSettings.Current.AutoPurgeEnabled = true;
            delay ??= new CountingDelayProbe();
            var snack = new CountingSnackbar();
            var toast = new CountingNotification();
            var svc = new PrintMonitorService(snack, toast, probe, delay);
            svc.SetMonitoredPrinters(new System.Collections.Generic.List<string> { Printer });
            return (svc, snack, toast, delay);
        }

        private static async Task RunPollsAsync(PrintMonitorService svc, CountingSnackbar _, CountingNotification __, int polls)
        {
            var method = typeof(PrintMonitorService).GetMethod("CheckPrintQueuesAsync", 
                System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance)!;
            for (int i = 0; i < polls; i++)
            {
                await (Task)method.Invoke(svc, new object[] { default(CancellationToken) })!;
            }
        }

        private const PrintJobStatus Error = PrintJobStatus.Error;
        private const PrintJobStatus Printing = PrintJobStatus.Printing;
        private const PrintJobStatus Offline = PrintJobStatus.Offline;
        private const PrintJobStatus Paused = PrintJobStatus.Paused;
        private const PrintJobStatus UserIntervention = PrintJobStatus.UserIntervention;

        private sealed class ScriptedProbe : IPrintQueueProbe
        {
            private readonly System.Collections.Generic.Queue<(JobSnapshot[] jobs, bool throwOnCancel)> _script = new();
            public int CancelCallCount { get; private set; }
            private bool _nextCancelShouldThrow;

            public void Queue(params JobSnapshot[] jobs) => _script.Enqueue((jobs, false));
            public void Queue(JobSnapshot[] jobs, bool throwOnCancel) => _script.Enqueue((jobs, throwOnCancel));
            public void Queue(JobSnapshot job, bool throwOnCancel) => _script.Enqueue((new[] { job }, throwOnCancel));

            public Task<System.Collections.Generic.IReadOnlyList<JobSnapshot>> GetJobsAsync(
                System.Collections.Generic.IEnumerable<string> monitoredPrinters, CancellationToken cancellationToken)
            {
                if (_script.Count == 0)
                {
                    _nextCancelShouldThrow = false;
                    return Task.FromResult<System.Collections.Generic.IReadOnlyList<JobSnapshot>>(
                        new System.Collections.Generic.List<JobSnapshot>());
                }
                var (jobs, throwOnCancel) = _script.Dequeue();
                _nextCancelShouldThrow = throwOnCancel;
                return Task.FromResult<System.Collections.Generic.IReadOnlyList<JobSnapshot>>(jobs);
            }

            public Task CancelAsync(int jobId, string printerName, CancellationToken cancellationToken)
            {
                CancelCallCount++;
                if (_nextCancelShouldThrow)
                {
                    _nextCancelShouldThrow = false;
                    throw new System.InvalidOperationException("job gone");
                }
                return Task.CompletedTask;
            }
        }

        private sealed class CountingDelayProbe : IDelayProbe
        {
            private readonly CancellationTokenSource? _cts;
            public CountingDelayProbe(CancellationTokenSource? cts = null)
            {
                _cts = cts;
            }

            public System.Collections.Generic.List<System.TimeSpan> Requests { get; } = new();
            public Task Delay(System.TimeSpan delay, CancellationToken cancellationToken)
            {
                Requests.Add(delay);
                _cts?.Cancel();
                return Task.CompletedTask;
            }
        }

        private sealed class CountingSnackbar : Wpf.Ui.ISnackbarService
        {
            public int Shown { get; private set; }
            public System.TimeSpan DefaultTimeOut { get; set; } = System.TimeSpan.FromSeconds(5);

            public void SetSnackbarPresenter(Wpf.Ui.Controls.SnackbarPresenter presenter) { }
            public Wpf.Ui.Controls.SnackbarPresenter GetSnackbarPresenter() => null!;

            public void Show(string title, string message, Wpf.Ui.Controls.ControlAppearance appearance,
                Wpf.Ui.Controls.IconElement? icon, System.TimeSpan duration)
            {
                Shown++;
            }
        }

        private sealed class CountingNotification : ShaPrint.WpfApp.Services.INotificationService
        {
            public int Shown { get; private set; }
            public void ShowPrinterError(string printerName, string errorDescription) => Shown++;
            public void ShowPrintJobCompleted(string documentName, string printerName) { }
            public void ShowPrintJobFailed(string documentName, string printerName, string reason) { }
            public void ShowClientConnected(string clientName) { }
            public void ShowClientDisconnected(string clientName) { }
            public void ShowScanCompleted(string fileName) { }
            public void ShowScanFailed(string errorMessage) { }
            public void ShowSecurityAlert(string message, string detail) { }
            public void ShowToast(string title, string body,
                ShaPrint.WpfApp.Services.ToastAction? action = null) { }
        }
    }
}
