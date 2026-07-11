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
            await listener.StopAsync();
            
            // Act: the old implementation would silently fail here due to MaxNumberOfServerInstances=1.
            // The fixed implementation awaits cleanup in Stop(), so the new Start() succeeds immediately.
            listener.Start();
            
            await Task.Delay(100);
            await listener.StopAsync();
            
            // Verify it is actually listening
            bool isListening = false;
            for (int i = 0; i < 20; i++)
            {
                if (listener.IsListening)
                {
                    isListening = true;
                    break;
                }
                await Task.Delay(100);
            }
            Assert.True(isListening, $"Listener failed to start listening. Last Error: {listener.LastError?.Message}");
        }
    }
}
