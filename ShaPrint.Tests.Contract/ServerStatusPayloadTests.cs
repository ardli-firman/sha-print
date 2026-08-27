using System.Reflection;
using System.Text.Json;
using ShaPrint.Core.Network;
using Xunit;

namespace ShaPrint.Tests.Contract;

/// <summary>
/// Contract tests for <see cref="ServerStatusPayload"/>.
///
/// MonitorPage (ShaPrint.UI) binds to HostName / Version / NetworkChannel /
/// UptimeSeconds / Printers / ActiveClients — this test locks that shape so a
/// payload change that would break the bindings (or the JSON wire format the
/// monitor parses) fails CI instead of failing silently at runtime.
/// </summary>
public class ServerStatusPayloadTests
{
    [Fact]
    public void Shape_ExposesMonitorBindingPropertiesWithCorrectTypes()
    {
        var type = typeof(ServerStatusPayload);

        Assert.Equal(typeof(string), type.GetProperty(nameof(ServerStatusPayload.ServerName))?.PropertyType);
        Assert.Equal(typeof(string), type.GetProperty(nameof(ServerStatusPayload.HostName))?.PropertyType);
        Assert.Equal(typeof(string), type.GetProperty(nameof(ServerStatusPayload.NetworkChannel))?.PropertyType);
        Assert.Equal(typeof(string), type.GetProperty(nameof(ServerStatusPayload.Version))?.PropertyType);
        Assert.Equal(typeof(long), type.GetProperty(nameof(ServerStatusPayload.UptimeSeconds))?.PropertyType);
        Assert.Equal(typeof(List<PrinterStatus>), type.GetProperty(nameof(ServerStatusPayload.Printers))?.PropertyType);
        Assert.Equal(typeof(List<ScannerStatus>), type.GetProperty(nameof(ServerStatusPayload.Scanners))?.PropertyType);
        Assert.Equal(typeof(List<ActiveClientInfo>), type.GetProperty(nameof(ServerStatusPayload.ActiveClients))?.PropertyType);
        Assert.Equal(typeof(List<JobHistoryEntry>), type.GetProperty(nameof(ServerStatusPayload.RecentJobs))?.PropertyType);
        Assert.Equal(typeof(List<ServerErrorEntry>), type.GetProperty(nameof(ServerStatusPayload.Errors))?.PropertyType);
    }

    [Fact]
    public void JsonRoundTrip_PreservesPayload()
    {
        var payload = new ServerStatusPayload
        {
            ServerName = "server-1",
            HostName = "host-1",
            NetworkChannel = "office",
            Version = "2.0.0",
            UptimeSeconds = 12_345,
            Printers = { new PrinterStatus { Name = "Epson L3210", Status = "online", QueueLength = 0 } },
            Scanners = { new ScannerStatus { Name = "Scanner X", Status = "available" } },
            ActiveClients = { new ActiveClientInfo { Ip = "192.168.1.50", ConnectedSince = DateTime.UtcNow } },
            RecentJobs =
            {
                new JobHistoryEntry
                {
                    Type = "print", Document = "invoice.pdf", PrinterName = "Epson L3210",
                    ClientIp = "192.168.1.50", Status = "completed", Timestamp = DateTime.UtcNow
                }
            },
            Errors = { new ServerErrorEntry { Source = "PrintMonitor", Message = "probe timeout", Timestamp = DateTime.UtcNow } }
        };

        var json = JsonSerializer.Serialize(payload);
        var deserialized = JsonSerializer.Deserialize<ServerStatusPayload>(json);

        Assert.NotNull(deserialized);
        Assert.Equal(payload.ServerName, deserialized.ServerName);
        Assert.Equal(payload.HostName, deserialized.HostName);
        Assert.Equal(payload.NetworkChannel, deserialized.NetworkChannel);
        Assert.Equal(payload.Version, deserialized.Version);
        Assert.Equal(payload.UptimeSeconds, deserialized.UptimeSeconds);

        Assert.Single(deserialized.Printers);
        Assert.Equal(payload.Printers[0].Name, deserialized.Printers[0].Name);
        Assert.Equal(payload.Printers[0].Status, deserialized.Printers[0].Status);
        Assert.Equal(payload.Printers[0].QueueLength, deserialized.Printers[0].QueueLength);

        Assert.Single(deserialized.Scanners);
        Assert.Equal(payload.Scanners[0].Name, deserialized.Scanners[0].Name);

        Assert.Single(deserialized.ActiveClients);
        Assert.Equal(payload.ActiveClients[0].Ip, deserialized.ActiveClients[0].Ip);
        Assert.Equal(payload.ActiveClients[0].ConnectedSince, deserialized.ActiveClients[0].ConnectedSince);

        Assert.Single(deserialized.RecentJobs);
        Assert.Equal(payload.RecentJobs[0].Document, deserialized.RecentJobs[0].Document);
        Assert.Equal(payload.RecentJobs[0].Status, deserialized.RecentJobs[0].Status);

        Assert.Single(deserialized.Errors);
        Assert.Equal(payload.Errors[0].Source, deserialized.Errors[0].Source);
        Assert.Equal(payload.Errors[0].Message, deserialized.Errors[0].Message);
    }
}
