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
                    bool isTargetJpeg = format.Equals("JPEG", StringComparison.OrdinalIgnoreCase) || 
                                        format.Equals("PDF", StringComparison.OrdinalIgnoreCase);

                    // Determine target format
                    PixelFormat targetFormat = colorMode switch
                    {
                        0 => isTargetJpeg ? PixelFormats.Gray8 : PixelFormats.BlackWhite,
                        1 => PixelFormats.Gray8,
                        _ => frame.Format
                    };

                    BitmapSource processedSource = frame;
                    if (frame.Format != targetFormat)
                    {
                        processedSource = new FormatConvertedBitmap(frame, targetFormat, null, 0);
                    }

                    if (colorMode == 0 && isTargetJpeg)
                    {
                        // Apply manual thresholding binarization to Gray8 bitmap
                        int width = processedSource.PixelWidth;
                        int height = processedSource.PixelHeight;
                        int stride = width; // Gray8 stride is 1 byte per pixel
                        byte[] pixels = new byte[height * stride];
                        processedSource.CopyPixels(pixels, stride, 0);

                        for (int i = 0; i < pixels.Length; i++)
                        {
                            pixels[i] = (pixels[i] < 128) ? (byte)0 : (byte)255;
                        }

                        processedSource = BitmapSource.Create(
                            width, height,
                            processedSource.DpiX, processedSource.DpiY,
                            PixelFormats.Gray8,
                            null,
                            pixels,
                            stride
                        );
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
                            jpegEncoder.QualityLevel = 95;
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
                // Fallback to original bytes if anything goes wrong
                return rawBytes;
            }
        }
    }
}
