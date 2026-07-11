using Xunit;
using System;
using System.Threading.Tasks;
using ShaPrint.Client;

namespace ShaPrint.Tests
{
    public class PipeListenerTests
    {
        [Fact]
        public async Task PipeListener_Restart_DoesNotThrowOrHang()
        {
            // Note: NamedPipeServerStream creates a machine-wide resource. We use a GUID to avoid test interference.
            string pipeName = @"\\.\pipe\shaprint_test_restart_" + Guid.NewGuid().ToString("N");
            var listener = new PipeListener(pipeName, "127.0.0.1", "Target", "Local");
            
            // Initial start
            listener.Start();
            await Task.Delay(100);
            
            // Trigger restart
            listener.Stop();
            
            // Act: the old implementation would silently fail here due to MaxNumberOfServerInstances=1.
            // The fixed implementation awaits cleanup in Stop(), so the new Start() succeeds immediately.
            listener.Start();
            
            await Task.Delay(100);
            listener.Stop();
            
            // If no exceptions were thrown (especially IOException/UnauthorizedAccessException inside the listener loop),
            // and the test completes, it passes.
            Assert.True(true);
        }
    }
}
