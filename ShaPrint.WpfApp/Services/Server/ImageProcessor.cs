using System;
using System.IO;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace ShaPrint.Server
{
    public static class ImageProcessor
    {
        public static byte[] ProcessImage(byte[] rawBytes, int colorMode, string format)
        {
            if (rawBytes == null || rawBytes.Length == 0)
                return Array.Empty<byte>();

            try
            {
                using (var ms = new MemoryStream(rawBytes))
                {
                    var decoder = BitmapDecoder.Create(ms, BitmapCreateOptions.None, BitmapCacheOption.OnLoad);
                    if (decoder.Frames.Count == 0)
                        return rawBytes;

                    var frame = decoder.Frames[0];
                    bool isTargetPng = format.Equals("PNG", StringComparison.OrdinalIgnoreCase);
                    bool isTargetPdf = format.Equals("PDF", StringComparison.OrdinalIgnoreCase);

                    BitmapSource processedSource;

                    if (colorMode == 2)
                    {
                        processedSource = ColorImageProcessor.Process(frame);
                    }
                    else if (colorMode == 1)
                    {
                        processedSource = GrayscaleImageProcessor.Process(frame);
                    }
                    else // B&W
                    {
                        processedSource = BlackAndWhiteImageProcessor.Process(frame, isTargetPng);
                    }

                    using (var outMs = new MemoryStream())
                    {
                        BitmapEncoder encoder;
                        if (isTargetPng)
                        {
                            encoder = new PngBitmapEncoder();
                        }
                        else
                        {
                            var jpegEncoder = new JpegBitmapEncoder();
                            jpegEncoder.QualityLevel = isTargetPdf ? 98 : 100;
                            encoder = jpegEncoder;
                        }
                        encoder.Frames.Add(BitmapFrame.Create(processedSource));
                        encoder.Save(outMs);
                        return outMs.ToArray();
                    }
                }
            }
            catch (Exception)
            {
                return rawBytes;
            }
        }
    }
}
