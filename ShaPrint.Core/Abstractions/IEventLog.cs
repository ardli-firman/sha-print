using System.Collections.Generic;

namespace ShaPrint.Core.Abstractions
{
    /// <summary>
    /// Represents a relevant event log entry (e.g. PrintService driver changed).
    /// </summary>
    public class EventLogEntry
    {
        public int EventId { get; set; }
        public string Source { get; set; } = string.Empty;
        public string Message { get; set; } = string.Empty;
        public System.DateTime TimeGenerated { get; set; }
    }

    /// <summary>
    /// Abstraction over Windows Event Log access.
    /// Enables testing cache invalidation without requiring actual event log access.
    /// </summary>
    public interface IEventLog
    {
        IEnumerable<EventLogEntry> GetEntries(string logName, int? eventId = null);
    }
}
