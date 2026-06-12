using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Server
{
    public class ScannerService
    {
        // WIA Format GUIDs
        private const string WiaFormatJPEG = "{B96B3CAE-0728-11D3-9D7B-0000F81EF32E}";
        private const string WiaFormatPNG = "{B96B3CAF-0728-11D3-9D7B-0000F81EF32E}";
        private const string WiaFormatTIFF = "{B96B3CB1-0728-11D3-9D7B-0000F81EF32E}";

        public List<ScannerInfo> GetLocalScanners()
        {
            var list = new List<ScannerInfo>();
            
            // WIA requires STA, so run listing on an STA thread
            var thread = new Thread(() =>
            {
                try
                {
                    Type? wiaType = Type.GetTypeFromProgID("WIA.DeviceManager");
                    if (wiaType == null)
                    {
                        AppLogger.Error("[SCANNER] WIA.DeviceManager is not registered (WIA not installed).");
                        return;
                    }

                    dynamic deviceManager = Activator.CreateInstance(wiaType)!;
                    foreach (dynamic info in deviceManager.DeviceInfos)
                    {
                        // Type == 1 means Scanner Device
                        if (info.Type == 1)
                        {
                            string friendlyName = GetScannerFriendlyName(info);
                            string desc = GetScannerDescription(info);

                            list.Add(new ScannerInfo
                            {
                                Name = friendlyName,
                                Description = desc
                            });
                        }
                    }
                }
                catch (Exception ex)
                {
                    AppLogger.Error("[SCANNER] Failed to list scanners", ex);
                }
            });

            thread.SetApartmentState(ApartmentState.STA);
            thread.Start();
            thread.Join();

            return list;
        }

        public byte[] PerformScan(string scannerName, int dpi, int colorMode, string format, out string actualFormat)
        {
            byte[] resultBytes = Array.Empty<byte>();
            string formatGuid = WiaFormatJPEG;
            string ext = "jpg";

            if (format.Equals("PNG", StringComparison.OrdinalIgnoreCase))
            {
                formatGuid = WiaFormatPNG;
                ext = "png";
            }
            else if (format.Equals("PDF", StringComparison.OrdinalIgnoreCase))
            {
                // PDF is wrapped on client or server using JPEG source
                formatGuid = WiaFormatJPEG;
                ext = "pdf";
            }

            actualFormat = ext;
            string outFormat = ext;
            byte[] rawBytes = Array.Empty<byte>();
            Exception? threadException = null;

            var thread = new Thread(() =>
            {
                try
                {
                    Type? wiaType = Type.GetTypeFromProgID("WIA.DeviceManager");
                    if (wiaType == null)
                        throw new InvalidOperationException("WIA is not installed on this machine.");

                    dynamic deviceManager = Activator.CreateInstance(wiaType)!;
                    dynamic? targetDeviceInfo = null;

                    foreach (dynamic info in deviceManager.DeviceInfos)
                    {
                        if (info.Type == 1) // Scanner
                        {
                            string friendlyName = GetScannerFriendlyName(info);
                            string deviceId = string.Empty;
                            try { deviceId = info.DeviceID?.ToString() ?? string.Empty; } catch { }

                            // Extract individual raw properties for backward compatibility matching
                            string rawName = string.Empty;
                            string rawDesc = string.Empty;
                            try { rawName = info.Properties[2].Value?.ToString() ?? string.Empty; } catch { }
                            try { rawDesc = info.Properties[3].Value?.ToString() ?? string.Empty; } catch { }
                            if (string.IsNullOrEmpty(rawName))
                            {
                                try { rawName = info.Properties["Name"].Value?.ToString() ?? string.Empty; } catch { }
                            }
                            if (string.IsNullOrEmpty(rawDesc))
                            {
                                try { rawDesc = info.Properties["Description"].Value?.ToString() ?? string.Empty; } catch { }
                            }

                            if (friendlyName.Equals(scannerName, StringComparison.OrdinalIgnoreCase) || 
                                rawName.Equals(scannerName, StringComparison.OrdinalIgnoreCase) || 
                                rawDesc.Equals(scannerName, StringComparison.OrdinalIgnoreCase) ||
                                deviceId.Equals(scannerName, StringComparison.OrdinalIgnoreCase))
                            {
                                targetDeviceInfo = info;
                                break;
                            }
                        }
                    }

                    if (targetDeviceInfo == null)
                        throw new DirectoryNotFoundException($"Scanner '{scannerName}' was not found.");

                    AppLogger.Log($"[SCANNER] Connecting to scanner: '{scannerName}'");
                    dynamic device = targetDeviceInfo.Connect();

                    if (device.Items.Count == 0)
                        throw new InvalidOperationException("Scanner has no scanning items.");

                    // Item 1 represents the scan bed / sensor
                    dynamic item = device.Items[1];

                    // Set scanning properties
                    // 4101: WIA_IPS_DOCUMENT_HANDLING_SELECT (1 = Flatbed)
                    SetWiaProperty(item.Properties, 4101, 1);
                    // 4103: WIA_IPS_XRESOLUTION
                    SetWiaProperty(item.Properties, 4103, dpi);
                    // 4104: WIA_IPS_YRESOLUTION
                    SetWiaProperty(item.Properties, 4104, dpi);
                    // 4102: WIA_IPA_DATATYPE (0 = B&W, 1 = Grayscale, 2 = Color)
                    SetWiaProperty(item.Properties, 4102, colorMode);

                    AppLogger.Log($"[SCANNER] Initiating scan: DPI={dpi}, ColorMode={colorMode}, Format={format}");
                    dynamic commonDialog = Activator.CreateInstance(Type.GetTypeFromProgID("WIA.CommonDialog")!)!;
                    
                    // ShowTransfer triggers the scan (CancelError = false)
                    dynamic imageFile = commonDialog.ShowTransfer(item, formatGuid, false);

                    if (imageFile == null)
                        throw new OperationCanceledException("Scan was cancelled or failed.");

                    // Save to a temporary file
                    string tempPath = Path.Combine(Path.GetTempPath(), $"shaprint_{Guid.NewGuid():N}.tmp");
                    try
                    {
                        imageFile.SaveFile(tempPath);
                        rawBytes = File.ReadAllBytes(tempPath);
                        AppLogger.Log($"[SCANNER] Scan completed successfully. Image size: {rawBytes.Length} bytes.");
                    }
                    finally
                    {
                        if (File.Exists(tempPath))
                        {
                            try { File.Delete(tempPath); } catch { }
                        }
                    }
                }
                catch (Exception ex)
                {
                    threadException = ex;
                }
            });

            thread.SetApartmentState(ApartmentState.STA);
            thread.Start();
            thread.Join();

            if (threadException != null)
            {
                throw threadException;
            }

            if (format.Equals("PDF", StringComparison.OrdinalIgnoreCase) && rawBytes.Length > 0)
            {
                AppLogger.Log("[SCANNER] Wrapping scanned JPEG bytes into PDF format.");
                return WrapJpegInPdf(rawBytes);
            }

            return rawBytes;
        }

        private static void SetWiaProperty(dynamic properties, object propIdOrName, object value)
        {
            try
            {
                dynamic prop = properties[propIdOrName];
                prop.set_Value(value);
            }
            catch (Exception ex)
            {
                AppLogger.Log($"[SCANNER] Warning: Failed to set property {propIdOrName}: {ex.Message}");
            }
        }

        /// <summary>
        /// Wraps raw JPEG bytes into a minimal compliant PDF/1.1 file structure.
        /// Avoids any external dependencies.
        /// </summary>
        private static byte[] WrapJpegInPdf(byte[] jpegBytes)
        {
            int width = 612; // default letter width in points
            int height = 792; // default letter height in points

            try
            {
                // Retrieve actual pixel dimensions using WPF BitmapDecoder
                using (var ms = new MemoryStream(jpegBytes))
                {
                    var decoder = System.Windows.Media.Imaging.BitmapDecoder.Create(
                        ms,
                        System.Windows.Media.Imaging.BitmapCreateOptions.None,
                        System.Windows.Media.Imaging.BitmapCacheOption.OnLoad);
                    if (decoder.Frames.Count > 0)
                    {
                        var frame = decoder.Frames[0];
                        // Convert pixels to PDF points (1 inch = 72 points, assuming 96 DPI screen default or raw size)
                        width = (int)Math.Round(frame.PixelWidth * 72.0 / 96.0);
                        height = (int)Math.Round(frame.PixelHeight * 72.0 / 96.0);
                    }
                }
            }
            catch (Exception ex)
            {
                AppLogger.Log($"[SCANNER] Warning: Could not parse image bounds for PDF, defaulting to letter size. {ex.Message}");
            }

            using (var ms = new MemoryStream())
            using (var sw = new StreamWriter(ms, System.Text.Encoding.ASCII))
            {
                sw.NewLine = "\n";
                
                // Write PDF Header
                sw.Write("%PDF-1.1\n");
                sw.Flush();
                
                long catalogOffset = ms.Position;
                sw.Write("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
                sw.Flush();

                long pagesOffset = ms.Position;
                sw.Write("2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
                sw.Flush();

                long pageOffset = ms.Position;
                sw.Write($"3 0 obj\n<< /Type /Page /Parent 2 0 R /Resources << /XObject << /Im1 4 0 R >> >> /Contents 5 0 R /MediaBox [0 0 {width} {height}] >>\nendobj\n");
                sw.Flush();

                long imageOffset = ms.Position;
                sw.Write($"4 0 obj\n<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {jpegBytes.Length} >>\nstream\n");
                sw.Flush();
                
                // Write binary image stream
                ms.Write(jpegBytes, 0, jpegBytes.Length);
                ms.Flush();
                
                sw.Write("\nendstream\nendobj\n");
                sw.Flush();

                long contentOffset = ms.Position;
                string contentStream = $"q\n{width} 0 0 {height} 0 0 cm\n/Im1 Do\nQ\n";
                sw.Write($"5 0 obj\n<< /Length {contentStream.Length} >>\nstream\n{contentStream}endstream\nendobj\n");
                sw.Flush();

                long xrefOffset = ms.Position;
                sw.Write("xref\n0 6\n");
                sw.Write("0000000000 65535 f \n");
                sw.Write($"{catalogOffset:D10} 00000 n \n");
                sw.Write($"{pagesOffset:D10} 00000 n \n");
                sw.Write($"{pageOffset:D10} 00000 n \n");
                sw.Write($"{imageOffset:D10} 00000 n \n");
                sw.Write($"{contentOffset:D10} 00000 n \n");
                sw.Flush();

                sw.Write($"trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xrefOffset}\n%%EOF\n");
                sw.Flush();

                return ms.ToArray();
            }
        }

        private static string GetScannerFriendlyName(dynamic info)
        {
            string deviceId = string.Empty;
            try { deviceId = info.DeviceID?.ToString() ?? string.Empty; } catch { }

            string manufacturer = string.Empty;
            string description = string.Empty;
            string name = string.Empty;

            try
            {
                foreach (dynamic prop in info.Properties)
                {
                    try
                    {
                        int propId = prop.PropertyID;
                        string propName = prop.Name;
                        object propVal = prop.Value;
                        if (propVal != null)
                        {
                            if (propId == 2 || propName.Equals("Unique Device ID", StringComparison.OrdinalIgnoreCase))
                                deviceId = propVal.ToString() ?? deviceId;
                            else if (propId == 3 || propName.Equals("Manufacturer", StringComparison.OrdinalIgnoreCase))
                                manufacturer = propVal.ToString() ?? string.Empty;
                            else if (propId == 4 || propName.Equals("Description", StringComparison.OrdinalIgnoreCase))
                                description = propVal.ToString() ?? string.Empty;
                            else if (propId == 7 || propName.Equals("Name", StringComparison.OrdinalIgnoreCase))
                                name = propVal.ToString() ?? string.Empty;
                        }
                    }
                    catch { }
                }
            }
            catch { }

            string friendlyName = string.Empty;

            // Prioritize Name (ID 7)
            if (!string.IsNullOrEmpty(name))
            {
                bool isGeneric = name.Equals("WIA Scanner", StringComparison.OrdinalIgnoreCase) || 
                                 name.Equals("WIA Scanner Device", StringComparison.OrdinalIgnoreCase) ||
                                 name.Equals("Scanner", StringComparison.OrdinalIgnoreCase);
                if (!isGeneric)
                {
                    friendlyName = name;
                }
            }

            // Fallback to Description (ID 4)
            if (string.IsNullOrEmpty(friendlyName) && !string.IsNullOrEmpty(description))
            {
                bool isGeneric = description.Equals("WIA Scanner", StringComparison.OrdinalIgnoreCase) || 
                                 description.Equals("WIA Scanner Device", StringComparison.OrdinalIgnoreCase) ||
                                 description.Equals("Scanner", StringComparison.OrdinalIgnoreCase);
                if (!isGeneric)
                {
                    friendlyName = description;
                }
            }

            // Fallback to Manufacturer + " Scanner"
            if (string.IsNullOrEmpty(friendlyName))
            {
                if (!string.IsNullOrEmpty(manufacturer) && !manufacturer.Equals("Microsoft", StringComparison.OrdinalIgnoreCase))
                {
                    friendlyName = manufacturer + " Scanner";
                }
                else
                {
                    friendlyName = !string.IsNullOrEmpty(name) ? name : (!string.IsNullOrEmpty(description) ? description : (string.IsNullOrEmpty(deviceId) ? "WIA Scanner" : deviceId));
                }
            }

            return friendlyName;
        }

        private static string GetScannerDescription(dynamic info)
        {
            string description = string.Empty;
            try
            {
                foreach (dynamic prop in info.Properties)
                {
                    try
                    {
                        int propId = prop.PropertyID;
                        string propName = prop.Name;
                        if (propId == 4 || propName.Equals("Description", StringComparison.OrdinalIgnoreCase))
                        {
                            description = prop.Value?.ToString() ?? string.Empty;
                            break;
                        }
                    }
                    catch { }
                }
            }
            catch { }

            if (string.IsNullOrEmpty(description))
            {
                try { description = info.Properties["Description"].Value?.ToString() ?? string.Empty; } catch { }
            }

            return !string.IsNullOrEmpty(description) ? description : "WIA Scanner Device";
        }
    }
}
