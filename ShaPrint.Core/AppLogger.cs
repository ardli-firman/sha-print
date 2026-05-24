using System;

namespace ShaPrint.Core
{
    public static class AppLogger
    {
        public static event Action<string>? OnLog;
        private static readonly object _lock = new object();

        public static void Log(string message)
        {
            string formattedMessage = $"[{DateTime.Now:HH:mm:ss}] {message}";
            
            lock (_lock)
            {
                Console.WriteLine(formattedMessage);
            }
            
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
