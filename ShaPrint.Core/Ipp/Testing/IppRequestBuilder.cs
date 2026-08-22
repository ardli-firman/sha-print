namespace ShaPrint.Core.Ipp.Testing;

/// <summary>
/// Helper to build IPP requests for testing.
/// Shared between unit tests and test console app.
/// </summary>
public static class IppRequestBuilder
{
    public static byte[] BuildGetPrinterAttributesRequest()
    {
        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms);

        writer.Write((byte)0x02); // version-major
        writer.Write((byte)0x00); // version-minor
        WriteBigEndian(writer, (short)0x000B); // Get-Printer-Attributes
        WriteBigEndian(writer, 1); // request-id

        writer.Write((byte)0x01); // begin-operation-attributes
        WriteStringAttribute(writer, 0x47, "attributes-charset", "utf-8");
        WriteStringAttribute(writer, 0x48, "attributes-natural-language", "en");
        WriteStringAttribute(writer, 0x45, "printer-uri", "ipp://localhost:631/printers");
        writer.Write((byte)0x03); // end-operation-attributes
        writer.Write((byte)0x03); // end-of-attributes

        return ms.ToArray();
    }

    public static byte[] BuildPrintJobRequest(string printerName, byte[] documentData)
    {
        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms);

        writer.Write((byte)0x02);
        writer.Write((byte)0x00);
        WriteBigEndian(writer, (short)0x0002); // Print-Job
        WriteBigEndian(writer, 1);

        writer.Write((byte)0x01);
        WriteStringAttribute(writer, 0x47, "attributes-charset", "utf-8");
        WriteStringAttribute(writer, 0x48, "attributes-natural-language", "en");
        WriteStringAttribute(writer, 0x45, "printer-uri", $"ipp://localhost:631/printers/{printerName}");
        WriteStringAttribute(writer, 0x42, "job-name", "TestJob");
        writer.Write((byte)0x03);

        // Document data
        writer.Write(documentData);
        writer.Write((byte)0x03);

        return ms.ToArray();
    }

    public static byte[] BuildGetJobsRequest()
    {
        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms);

        writer.Write((byte)0x02);
        writer.Write((byte)0x00);
        WriteBigEndian(writer, (short)0x000A); // Get-Jobs
        WriteBigEndian(writer, 1);

        writer.Write((byte)0x01);
        WriteStringAttribute(writer, 0x47, "attributes-charset", "utf-8");
        WriteStringAttribute(writer, 0x48, "attributes-natural-language", "en");
        WriteStringAttribute(writer, 0x45, "printer-uri", "ipp://localhost:631/printers");
        writer.Write((byte)0x03);
        writer.Write((byte)0x03);

        return ms.ToArray();
    }

    public static byte[] BuildCancelJobRequest(int jobId)
    {
        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms);

        writer.Write((byte)0x02);
        writer.Write((byte)0x00);
        WriteBigEndian(writer, (short)0x0008); // Cancel-Job
        WriteBigEndian(writer, 1);

        writer.Write((byte)0x01);
        WriteStringAttribute(writer, 0x47, "attributes-charset", "utf-8");
        WriteStringAttribute(writer, 0x48, "attributes-natural-language", "en");
        WriteStringAttribute(writer, 0x45, "printer-uri", "ipp://localhost:631/printers");
        writer.Write((byte)0x21); // integer
        WriteBigEndian(writer, (short)0x0006); // name-length
        writer.Write("job-id"u8.ToArray());
        WriteBigEndian(writer, (short)0x0004); // value-length
        WriteBigEndian(writer, jobId);
        writer.Write((byte)0x03);
        writer.Write((byte)0x03);

        return ms.ToArray();
    }

    public static byte[] BuildGetJobAttributesRequest(int jobId)
    {
        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms);

        writer.Write((byte)0x02);
        writer.Write((byte)0x00);
        WriteBigEndian(writer, (short)0x0009); // Get-Job-Attributes
        WriteBigEndian(writer, 1);

        writer.Write((byte)0x01);
        WriteStringAttribute(writer, 0x47, "attributes-charset", "utf-8");
        WriteStringAttribute(writer, 0x48, "attributes-natural-language", "en");
        WriteStringAttribute(writer, 0x45, "printer-uri", "ipp://localhost:631/printers");
        writer.Write((byte)0x21); // integer
        WriteBigEndian(writer, (short)0x0006); // name-length
        writer.Write("job-id"u8.ToArray());
        WriteBigEndian(writer, (short)0x0004); // value-length
        WriteBigEndian(writer, jobId);
        writer.Write((byte)0x03);
        writer.Write((byte)0x03);

        return ms.ToArray();
    }

    public static byte[] BuildValidateJobRequest(string printerName)
    {
        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms);

        writer.Write((byte)0x02);
        writer.Write((byte)0x00);
        WriteBigEndian(writer, (short)0x0004); // Validate-Job
        WriteBigEndian(writer, 1);

        writer.Write((byte)0x01);
        WriteStringAttribute(writer, 0x47, "attributes-charset", "utf-8");
        WriteStringAttribute(writer, 0x48, "attributes-natural-language", "en");
        WriteStringAttribute(writer, 0x45, "printer-uri", $"ipp://localhost:631/printers/{printerName}");
        writer.Write((byte)0x03);
        writer.Write((byte)0x03);

        return ms.ToArray();
    }

    public static byte[] BuildInvalidVersionRequest()
    {
        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms);

        // Invalid version
        writer.Write((byte)0x00); // version-major (invalid)
        writer.Write((byte)0x00); // version-minor (invalid)
        WriteBigEndian(writer, (short)0x000B); // Get-Printer-Attributes
        WriteBigEndian(writer, 1); // request-id

        // Minimal attributes
        writer.Write((byte)0x01); // begin-operation-attributes
        WriteStringAttribute(writer, 0x47, "attributes-charset", "utf-8");
        writer.Write((byte)0x03); // end-operation-attributes
        writer.Write((byte)0x03); // end-of-attributes

        return ms.ToArray();
    }

    private static void WriteStringAttribute(BinaryWriter writer, byte tag, string name, string value)
    {
        writer.Write(tag);
        var nameBytes = System.Text.Encoding.UTF8.GetBytes(name);
        WriteBigEndian(writer, (short)nameBytes.Length);
        writer.Write(nameBytes);
        var valueBytes = System.Text.Encoding.UTF8.GetBytes(value);
        WriteBigEndian(writer, (short)valueBytes.Length);
        writer.Write(valueBytes);
    }

    private static void WriteBigEndian(BinaryWriter writer, short value)
    {
        writer.Write((byte)(value >> 8));
        writer.Write((byte)(value & 0xFF));
    }

    private static void WriteBigEndian(BinaryWriter writer, int value)
    {
        writer.Write((byte)(value >> 24));
        writer.Write((byte)((value >> 16) & 0xFF));
        writer.Write((byte)((value >> 8) & 0xFF));
        writer.Write((byte)(value & 0xFF));
    }
}
