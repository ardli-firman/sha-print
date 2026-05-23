using System.IO;
using System.Text;
using System.Threading.Tasks;

namespace ShaPrint.Core.Network
{
    public class PrintJobPayload
    {
        public string TargetPrinterName { get; set; } = string.Empty;
        public byte[] SpoolData { get; set; } = new byte[0];

        public static async Task WriteAsync(Stream stream, PrintJobPayload payload)
        {
            using var writer = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true);
            writer.Write(payload.TargetPrinterName);
            writer.Write(payload.SpoolData.Length);
            writer.Write(payload.SpoolData);
            writer.Flush();
        }

        public static async Task<PrintJobPayload> ReadAsync(Stream stream)
        {
            using var reader = new BinaryReader(stream, Encoding.UTF8, leaveOpen: true);
            var payload = new PrintJobPayload();
            payload.TargetPrinterName = reader.ReadString();
            int dataLength = reader.ReadInt32();
            payload.SpoolData = reader.ReadBytes(dataLength);
            return payload;
        }
    }
}
