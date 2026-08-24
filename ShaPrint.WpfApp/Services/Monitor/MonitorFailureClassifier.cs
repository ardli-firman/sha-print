using System;
using System.IO;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text.Json;
using ShaPrint.Core.Network;

namespace ShaPrint.WpfApp.Services.Monitor;

public enum MonitorFailureCategory
{
    AuthMismatch,
    ProtocolError,
    Overloaded,
    Unreachable
}

internal static class MonitorFailureClassifier
{
    internal static MonitorFailureCategory Classify(Exception exception)
        => exception switch
        {
            MonitorAuthenticationFailedException => MonitorFailureCategory.AuthMismatch,
            MonitorOverloadedException => MonitorFailureCategory.Overloaded,
            CryptographicException => MonitorFailureCategory.AuthMismatch,
            JsonException or InvalidDataException or EndOfStreamException
                => MonitorFailureCategory.ProtocolError,
            SocketException or IOException or TimeoutException or OperationCanceledException
                => MonitorFailureCategory.Unreachable,
            _ => MonitorFailureCategory.ProtocolError
        };
}
