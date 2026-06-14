using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using ShaPrint.Core;
using ShaPrint.Core.Network;

namespace ShaPrint.Server
{
    public class ScannerService
    {
        // WIA Format GUIDs
        private const string WiaFormatBMP = "{B96B3CAB-0728-11D3-9D7B-0000F81EF32E}";
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

        public byte[] PerformScan(string scannerName, int dpi, int colorMode, string format, int brightness, int contrast, out string actualFormat)
        {
            byte[] resultBytes = Array.Empty<byte>();
            string ext = "jpg";
 
            if (format.Equals("PNG", StringComparison.OrdinalIgnoreCase))
            {
                ext = "png";
            }
            else if (format.Equals("PDF", StringComparison.OrdinalIgnoreCase))
            {
                ext = "pdf";
            }
 
            actualFormat = ext;
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
 
                    // Set scanning properties on the device level
                    // 3088: WIA_DPS_DOCUMENT_HANDLING_SELECT (1 = Flatbed)
                    SetWiaProperty(device.Properties, 3088, 1);
 
                    // 1. Set intent to 0 (None) first to prevent the driver from resetting other properties
                    SetWiaProperty(item.Properties, 6146, 0);
 
                    // 2. Set DataType and Depth first, because changing them resets resolution
                    int wiaDataType = colorMode switch
                    {
                        0 => 0, // WIA_DATA_THRESHOLD
                        1 => 2, // WIA_DATA_GRAYSCALE
                        _ => 3  // WIA_DATA_COLOR
                    };
                    SetWiaProperty(item.Properties, 4103, wiaDataType);
 
                    int wiaDepth = colorMode switch
                    {
                        0 => 1,
                        1 => 8,
                        _ => 24
                    };
                    SetWiaProperty(item.Properties, 4104, wiaDepth);
 
                    // 3. Set Resolution (DPI) AFTER DataType/Depth
                    SetWiaProperty(item.Properties, 6147, dpi);
                    SetWiaProperty(item.Properties, 6148, dpi);
 
                    // 4. Set Extents (DPI-aware size)
                    double bedWidthInches = 8.5;  // Default Letter width
                    double bedHeightInches = 11.0; // Default Letter height
                    object? widthVal = GetWiaPropertyValue(item.Properties, 6165);
                    if (widthVal != null)
                    {
                        try
                        {
                            int widthThousandths = Convert.ToInt32(widthVal);
                            if (widthThousandths > 0)
                                bedWidthInches = widthThousandths / 1000.0;
                        }
                        catch { }
                    }
 
                    object? heightVal = GetWiaPropertyValue(item.Properties, 6166);
                    if (heightVal != null)
                    {
                        try
                        {
                            int heightThousandths = Convert.ToInt32(heightVal);
                            if (heightThousandths > 0)
                                bedHeightInches = heightThousandths / 1000.0;
                        }
                        catch { }
                    }
 
                    int widthPixels = (int)Math.Round(bedWidthInches * dpi);
                    int heightPixels = (int)Math.Round(bedHeightInches * dpi);
 
                    // 6149: WIA_IPS_XPOS, 6150: WIA_IPS_YPOS (start positions at 0)
                    SetWiaProperty(item.Properties, 6149, 0);
                    SetWiaProperty(item.Properties, 6150, 0);
                    // 6151: WIA_IPS_XEXTENT, 6152: WIA_IPS_YEXTENT (pixel dimensions)
                    SetWiaProperty(item.Properties, 6151, widthPixels);
                    SetWiaProperty(item.Properties, 6152, heightPixels);
 
                    // 5. Set Quality Sliders (Brightness and Contrast scaled from -100..100 to -1000..1000)
                    int wiaBrightness = Math.Clamp(brightness * 10, -1000, 1000);
                    int wiaContrast = Math.Clamp(contrast * 10, -1000, 1000);
                    SetWiaProperty(item.Properties, 6154, wiaBrightness);
                    SetWiaProperty(item.Properties, 6155, wiaContrast);
 
                    // 6. Diagnostic read-back logging
                    try
                    {
                        object? actualDpiX = GetWiaPropertyValue(item.Properties, 6147);
                        object? actualDpiY = GetWiaPropertyValue(item.Properties, 6148);
                        object? actualExtentX = GetWiaPropertyValue(item.Properties, 6151);
                        object? actualExtentY = GetWiaPropertyValue(item.Properties, 6152);
                        AppLogger.Log($"[SCANNER] Applied WIA settings: DPI={actualDpiX}x{actualDpiY}, Extent={actualExtentX}x{actualExtentY}");
                    }
                    catch (Exception ex)
                    {
                        AppLogger.Log($"[SCANNER] Failed to read back WIA settings: {ex.Message}");
                    }
 
                    AppLogger.Log($"[SCANNER] Initiating scan: DPI={dpi}, Bed={bedWidthInches}x{bedHeightInches}\", Size={widthPixels}x{heightPixels}px, ColorMode={colorMode}, Brightness={wiaBrightness}, Contrast={wiaContrast}, Format={format}");
                    
                    dynamic? imageFile = null;
                    try
                    {
                        AppLogger.Log("[SCANNER] Attempting silent transfer using item.Transfer(TIFF).");
                        imageFile = item.Transfer(WiaFormatTIFF);
                    }
                    catch (Exception ex)
                    {
                        AppLogger.Log($"[SCANNER] Silent TIFF transfer failed: {ex.Message}. Trying BMP.");
                        try
                        {
                            imageFile = item.Transfer(WiaFormatBMP);
                        }
                        catch (Exception ex2)
                        {
                            AppLogger.Log($"[SCANNER] Silent BMP transfer failed: {ex2.Message}. Falling back to CommonDialog.");
                            dynamic commonDialog = Activator.CreateInstance(Type.GetTypeFromProgID("WIA.CommonDialog")!)!;
                            try
                            {
                                imageFile = commonDialog.ShowTransfer(item, WiaFormatTIFF, false);
                            }
                            catch (Exception ex3)
                            {
                                AppLogger.Log($"[SCANNER] CommonDialog TIFF transfer failed: {ex3.Message}. Trying BMP.");
                                imageFile = commonDialog.ShowTransfer(item, WiaFormatBMP, false);
                            }
                        }
                    }
 
                    if (imageFile == null)
                        throw new OperationCanceledException("Scan was cancelled or failed.");
  
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
 
            if (rawBytes.Length > 0)
            {
                // Apply our robust Post-Processing logic to guarantee target format and color mode
                rawBytes = ImageProcessor.ProcessImage(rawBytes, colorMode, format);
            }
 
            if (format.Equals("PDF", StringComparison.OrdinalIgnoreCase) && rawBytes.Length > 0)
            {
                AppLogger.Log("[SCANNER] Wrapping scanned JPEG bytes into PDF format.");
                return WrapJpegInPdf(rawBytes);
            }
 
            return rawBytes;
        }

        private static void SetWiaProperty(dynamic properties, int propId, object value)
        {
            try
            {
                dynamic? prop = null;
                foreach (dynamic p in properties)
                {
                    try
                    {
                        if (p.PropertyID == propId)
                        {
                            prop = p;
                            break;
                        }
                    }
                    catch { }
                }

                if (prop != null)
                {
                    prop.set_Value(value);
                }
                else
                {
                    AppLogger.Log($"[SCANNER] Warning: Property with ID {propId} not found in collection.");
                }
            }
            catch (Exception ex)
            {
                AppLogger.Log($"[SCANNER] Warning: Failed to set property with ID {propId}: {ex.Message}");
            }
        }

        private static object? GetWiaPropertyValue(dynamic properties, int propId)
        {
            try
            {
                foreach (dynamic p in properties)
                {
                    try
                    {
                        if (p.PropertyID == propId)
                        {
                            return p.Value;
                        }
                    }
                    catch { }
                }
            }
            catch (Exception ex)
            {
                AppLogger.Log($"[SCANNER] Warning: Failed to get property with ID {propId}: {ex.Message}");
            }
            return null;
        }

        /// <summary>
        /// Wraps raw JPEG bytes into a minimal compliant PDF/1.1 file structure.
        /// Avoids any external dependencies.
        /// </summary>
        public static byte[] WrapJpegInPdf(byte[] jpegBytes)
        {
            int pixelWidth = 0;
            int pixelHeight = 0;
            int pointsWidth = 612; // default letter width in points
            int pointsHeight = 792; // default letter height in points
            string colorSpace = "DeviceRGB";

            try
            {
                // Retrieve actual pixel dimensions using WPF BitmapDecoder
                using (var ms = new MemoryStream(jpegBytes))
                {
                    var decoder = BitmapDecoder.Create(
                        ms,
                        System.Windows.Media.Imaging.BitmapCreateOptions.None,
                        System.Windows.Media.Imaging.BitmapCacheOption.OnLoad);
                    if (decoder.Frames.Count > 0)
                    {
                        var frame = decoder.Frames[0];
                        pixelWidth = frame.PixelWidth;
                        pixelHeight = frame.PixelHeight;

                        double dpiX = frame.DpiX;
                        double dpiY = frame.DpiY;
                        if (dpiX <= 10 || dpiX > 4800) dpiX = 96.0;
                        if (dpiY <= 10 || dpiY > 4800) dpiY = 96.0;

                        // Convert pixels to PDF points (1 inch = 72 points) using image DPI
                        pointsWidth = (int)Math.Round(frame.PixelWidth * 72.0 / dpiX);
                        pointsHeight = (int)Math.Round(frame.PixelHeight * 72.0 / dpiY);

                        var format = frame.Format;
                        if (format == System.Windows.Media.PixelFormats.Gray8 ||
                            format == System.Windows.Media.PixelFormats.BlackWhite ||
                            format == System.Windows.Media.PixelFormats.Gray2 ||
                            format == System.Windows.Media.PixelFormats.Gray4 ||
                            format == System.Windows.Media.PixelFormats.Indexed1 ||
                            format == System.Windows.Media.PixelFormats.Indexed2 ||
                            format == System.Windows.Media.PixelFormats.Indexed4 ||
                            format == System.Windows.Media.PixelFormats.Indexed8)
                        {
                            colorSpace = "DeviceGray";
                        }
                    }
                }
            }
            catch (Exception ex)
            {
                AppLogger.Log($"[SCANNER] Warning: Could not parse image bounds/color space for PDF, defaulting to letter size RGB. {ex.Message}");
            }

            if (pixelWidth <= 0) pixelWidth = pointsWidth;
            if (pixelHeight <= 0) pixelHeight = pointsHeight;

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
                sw.Write($"3 0 obj\n<< /Type /Page /Parent 2 0 R /Resources << /XObject << /Im1 4 0 R >> >> /Contents 5 0 R /MediaBox [0 0 {pointsWidth} {pointsHeight}] >>\nendobj\n");
                sw.Flush();
 
                long imageOffset = ms.Position;
                sw.Write($"4 0 obj\n<< /Type /XObject /Subtype /Image /Width {pixelWidth} /Height {pixelHeight} /ColorSpace /{colorSpace} /BitsPerComponent 8 /Filter /DCTDecode /Length {jpegBytes.Length} >>\nstream\n");
                sw.Flush();
                
                // Write binary image stream
                ms.Write(jpegBytes, 0, jpegBytes.Length);
                ms.Flush();
                
                sw.Write("\nendstream\nendobj\n");
                sw.Flush();
 
                long contentOffset = ms.Position;
                string contentStream = $"q\n{pointsWidth} 0 0 {pointsHeight} 0 0 cm\n/Im1 Do\nQ\n";
                sw.Write($"5 0 obj\n<< /Length {contentStream.Length} >>\nstream\n{contentStream}endstream\nendobj\n");
                sw.Flush();
 
                long xrefOffset = ms.Position;
                sw.Write("xref\n0 6\n");
                sw.Write("0000000000 65535 f\r\n");
                sw.Write($"{catalogOffset:D10} 00000 n\r\n");
                sw.Write($"{pagesOffset:D10} 00000 n\r\n");
                sw.Write($"{pageOffset:D10} 00000 n\r\n");
                sw.Write($"{imageOffset:D10} 00000 n\r\n");
                sw.Write($"{contentOffset:D10} 00000 n\r\n");
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
