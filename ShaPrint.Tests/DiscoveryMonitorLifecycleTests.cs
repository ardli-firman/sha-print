using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using ShaPrint.Client;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Server;
using ShaPrint.WpfApp.Services;
using ShaPrint.WpfApp.Services.Monitor;
using ShaPrint.WpfApp.Services.Server;
using ShaPrint.WpfApp.ViewModels.Pages;
using Xunit;

namespace ShaPrint.Tests;

[Collection("SequentialNetworkTests")]
public sealed class DiscoveryMonitorLifecycleTests
{
    [Fact]
    public async Task DiscoveryClient_ExternalCancellation_StopsPromptly()
    {
        var client = new DiscoveryClient();
        using var cancellation = new CancellationTokenSource(TimeSpan.FromMilliseconds(100));

        var started = DateTime.UtcNow;
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() =>
            client.DiscoverServersAsync(
                targetIp: "127.0.0.1",
                timeoutMs: 10_000,
                skipUnicastSweep: true,
                cancellationToken: cancellation.Token));

        Assert.True(DateTime.UtcNow - started < TimeSpan.FromSeconds(2));
    }

    [Fact]
    public async Task DiscoveryServer_StartAndStop_AreIdempotentAndAwaitable()
    {
        var server = new DiscoveryServer(new NullNotificationService());

        server.Start();
        server.Start();
        await server.StopAsync();
        await server.StopAsync();
    }

    [Fact]
    public void DiscoveryClient_OversizedResponse_IsRejected()
    {
        byte[] oversized = new byte[Constants.MaxDiscoveryResponseBytes + 1];

        bool accepted = DiscoveryClient.TryParseResponse(
            oversized,
            new IPEndPoint(IPAddress.Loopback, Constants.DiscoveryUdpPort),
            out var response);

        Assert.False(accepted);
        Assert.Null(response);
    }

    [Fact]
    public async Task DiscoveryServer_SetExposedPrinters_TakesImmutableSnapshot()
    {
        var exposed = new List<string> { "Printer A" };
        var server = new DiscoveryServer(new NullNotificationService());
        server.SetDriverSharingEnabled(false);
        server.SetExposedPrinters(exposed);
        exposed[0] = "MUTATED";
        exposed.Add("Printer B");

        try
        {
            server.Start();
            var client = new DiscoveryClient();
            var responses = await client.DiscoverServersAsync(
                targetIp: "127.0.0.1",
                timeoutMs: 750,
                skipUnicastSweep: true);

            var response = Assert.Single(responses);
            Assert.Single(response.ExposedPrinters);
            Assert.Equal("Printer A", response.ExposedPrinters[0].Name);
        }
        finally
        {
            await server.StopAsync();
        }
    }

    [Fact]
    public async Task MonitorFrameCodec_TruncatedBody_IsRejected()
    {
        byte[] frame = new byte[sizeof(int) + 2];
        BinaryPrimitives.WriteInt32LittleEndian(frame, 4);
        using var stream = new MemoryStream(frame);

        await Assert.ThrowsAsync<EndOfStreamException>(() =>
            MonitorFrameCodec.ReadAsync(stream, maxPayloadBytes: 16, CancellationToken.None));
    }

    [Fact]
    public async Task MonitorFrameCodec_OversizedBody_IsRejectedBeforeAllocation()
    {
        byte[] header = new byte[sizeof(int)];
        BinaryPrimitives.WriteInt32LittleEndian(header, 17);
        using var stream = new MemoryStream(header);

        await Assert.ThrowsAsync<InvalidDataException>(() =>
            MonitorFrameCodec.ReadAsync(stream, maxPayloadBytes: 16, CancellationToken.None));
    }

    [Theory]
    [InlineData(typeof(CryptographicException), MonitorFailureCategory.AuthMismatch)]
    [InlineData(typeof(InvalidDataException), MonitorFailureCategory.ProtocolError)]
    [InlineData(typeof(System.Text.Json.JsonException), MonitorFailureCategory.ProtocolError)]
    [InlineData(typeof(SocketException), MonitorFailureCategory.Unreachable)]
    [InlineData(typeof(TimeoutException), MonitorFailureCategory.Unreachable)]
    public void MonitorFailureClassifier_MapsDistinctCategories(
        Type exceptionType,
        MonitorFailureCategory expected)
    {
        var exception = (Exception)Activator.CreateInstance(exceptionType)!;

        Assert.Equal(expected, MonitorFailureClassifier.Classify(exception));
    }

    [Theory]
    [InlineData("ProtocolError")]
    [InlineData("Overloaded")]
    public void MonitorViewModel_UpdateServerFailure_PreservesDistinctStatus(string category)
    {
        var viewModel = new MonitorViewModel();
        viewModel.Servers.Add(new ServerNode
        {
            HostName = "SERVER-1",
            IpAddress = "10.0.0.1",
            Status = "Online",
            LastSeen = DateTime.UtcNow,
            Payload = new ServerStatusPayload()
        });

        viewModel.UpdateServerFailure("SERVER-1", "10.0.0.1", category);

        Assert.Equal(category, viewModel.Servers[0].Status);
    }

    [Fact]
    public async Task MonitorTcpServer_StartAndStop_AreIdempotentAndAwaitable()
    {
        var server = new MonitorTcpServer(new ServerStatusProvider(null!));

        server.Start();
        server.Start();
        await server.StopAsync();
        await server.StopAsync();
    }

    [Fact]
    public async Task MonitorService_StartAndStop_AreIdempotentAndAwaitable()
    {
        var service = new MonitorService(new MonitorViewModel());

        service.Start();
        service.Start();
        await service.StopAsync();
        await service.StopAsync();
    }

    private sealed class NullNotificationService : INotificationService
    {
        public void ShowClientConnected(string clientIp) { }
        public void ShowClientDisconnected(string clientIp) { }
        public void ShowPrintJobCompleted(string documentName, string printerName) { }
        public void ShowPrintJobFailed(string documentName, string printerName, string reason) { }
        public void ShowScanCompleted(string fileName) { }
        public void ShowScanFailed(string errorMessage) { }
        public void ShowPrinterError(string printerName, string errorDescription) { }
        public void ShowSecurityAlert(string message, string detail) { }
        public void ShowToast(string title, string body, ToastAction? action = null) { }
    }
}
