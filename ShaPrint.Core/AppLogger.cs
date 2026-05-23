using System;

namespace ShaPrint.Core
{
    public static class AppLogger
    {
        public static event Action<string>? OnLog;

        public static void Log(string message)
        {
            string formattedMessage = $"[{DateTime.Now:HH:mm:ss}] {message}";
            // Still write to console for background/service usage or debugging
            Console.WriteLine(formattedMessage);
            
            // Fire event for UI
            OnLog?.Invoke(formattedMessage);
        }
        
        public static void Error(string message, Exception? ex = null)
        {
            string errorMsg = ex == null ? $"[ERROR] {message}" : $"[ERROR] {message}\n{ex}";
            Log(errorMsg);
        }
    }
}
