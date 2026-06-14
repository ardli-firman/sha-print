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

                    BitmapSource processedSource = frame;

                    if (colorMode == 2)
                    {
                        // Color: convert to Bgr24
                        if (processedSource.Format != PixelFormats.Bgr24)
                        {
                            processedSource = new FormatConvertedBitmap(processedSource, PixelFormats.Bgr24, null, 0);
                        }
                        // Smooth (denoise) to remove scanner grid noise, then apply Laplacian sharpening
                        processedSource = SmoothBgr24(processedSource);
                        processedSource = SharpenBgr24(processedSource);
                    }
                    else if (colorMode == 1)
                    {
                        // Grayscale: convert to Gray8
                        if (processedSource.Format != PixelFormats.Gray8)
                        {
                            processedSource = new FormatConvertedBitmap(processedSource, PixelFormats.Gray8, null, 0);
                        }
                        // Smooth (denoise), then apply Laplacian sharpening
                        processedSource = SmoothGray8(processedSource);
                        processedSource = SharpenGray8(processedSource);
                    }
                    else // B&W
                    {
                        // B&W: convert to Gray8
                        if (processedSource.Format != PixelFormats.Gray8)
                        {
                            processedSource = new FormatConvertedBitmap(processedSource, PixelFormats.Gray8, null, 0);
                        }
                        
                        // 1. Smooth (denoise) to remove paper noise/dust
                        processedSource = SmoothGray8(processedSource);
                        
                        // 2. Sharpen (Laplacian) to make signature edges crisp
                        processedSource = SharpenGray8(processedSource);

                        int width = processedSource.PixelWidth;
                        int height = processedSource.PixelHeight;
                        int stride = width;
                        byte[] pixels = new byte[height * stride];
                        processedSource.CopyPixels(pixels, stride, 0);

                        // 3. Soft Threshold (anti-aliased binarization)
                        // Range [135, 210] cleans background, sets ink to solid black,
                        // and leaves a smooth gray transition at edges to prevent pixelation.
                        for (int i = 0; i < pixels.Length; i++)
                        {
                            int val = pixels[i];
                            if (val < 135)
                                pixels[i] = 0;
                            else if (val > 210)
                                pixels[i] = 255;
                            else
                                pixels[i] = (byte)((val - 135) * 255 / (210 - 135));
                        }

                        var bwSource = BitmapSource.Create(
                            width, height,
                            processedSource.DpiX, processedSource.DpiY,
                            PixelFormats.Gray8,
                            null,
                            pixels,
                            stride
                        );

                        // Keep as Gray8 to preserve anti-aliased smooth borders (do NOT convert to 1-bit BlackWhite)
                        processedSource = bwSource;
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

        private static BitmapSource SmoothGray8(BitmapSource source)
        {
            try
            {
                int width = source.PixelWidth;
                int height = source.PixelHeight;
                int stride = width;
                byte[] srcPixels = new byte[height * stride];
                source.CopyPixels(srcPixels, stride, 0);

                byte[] destPixels = new byte[height * stride];
                Array.Copy(srcPixels, destPixels, srcPixels.Length);

                // 3x3 Mean filter (box blur) for smoothing
                for (int y = 1; y < height - 1; y++)
                {
                    int rowOffset = y * stride;
                    for (int x = 1; x < width - 1; x++)
                    {
                        int sum = 0;
                        for (int ky = -1; ky <= 1; ky++)
                        {
                            int kRowOffset = (y + ky) * stride;
                            for (int kx = -1; kx <= 1; kx++)
                            {
                                sum += srcPixels[kRowOffset + (x + kx)];
                            }
                        }
                        destPixels[rowOffset + x] = (byte)(sum / 9);
                    }
                }

                return BitmapSource.Create(
                    width, height,
                    source.DpiX, source.DpiY,
                    PixelFormats.Gray8,
                    null,
                    destPixels,
                    stride
                );
            }
            catch
            {
                return source;
            }
        }

        private static BitmapSource SmoothBgr24(BitmapSource source)
        {
            try
            {
                int width = source.PixelWidth;
                int height = source.PixelHeight;
                int stride = ((width * 3 + 3) / 4) * 4;
                byte[] srcPixels = new byte[height * stride];
                source.CopyPixels(srcPixels, stride, 0);

                byte[] destPixels = new byte[height * stride];
                Array.Copy(srcPixels, destPixels, srcPixels.Length);

                // 3x3 Mean filter (box blur) for Bgr24 smoothing
                for (int y = 1; y < height - 1; y++)
                {
                    int rowOffset = y * stride;
                    for (int x = 1; x < width - 1; x++)
                    {
                        int idx = rowOffset + x * 3;
                        int sumB = 0, sumG = 0, sumR = 0;

                        for (int ky = -1; ky <= 1; ky++)
                        {
                            int kRowOffset = (y + ky) * stride;
                            for (int kx = -1; kx <= 1; kx++)
                            {
                                int kIdx = kRowOffset + (x + kx) * 3;
                                sumB += srcPixels[kIdx];
                                sumG += srcPixels[kIdx + 1];
                                sumR += srcPixels[kIdx + 2];
                            }
                        }

                        destPixels[idx] = (byte)(sumB / 9);
                        destPixels[idx + 1] = (byte)(sumG / 9);
                        destPixels[idx + 2] = (byte)(sumR / 9);
                    }
                }

                return BitmapSource.Create(
                    width, height,
                    source.DpiX, source.DpiY,
                    PixelFormats.Bgr24,
                    null,
                    destPixels,
                    stride
                );
            }
            catch
            {
                return source;
            }
        }

        private static BitmapSource SharpenGray8(BitmapSource source)
        {
            try
            {
                int width = source.PixelWidth;
                int height = source.PixelHeight;
                int stride = width;
                byte[] srcPixels = new byte[height * stride];
                source.CopyPixels(srcPixels, stride, 0);

                byte[] destPixels = new byte[height * stride];
                Array.Copy(srcPixels, destPixels, srcPixels.Length);

                // Laplacian 3x3 sharpening filter:
                // [  0, -1,  0 ]
                // [ -1,  5, -1 ]
                // [  0, -1,  0 ]
                for (int y = 1; y < height - 1; y++)
                {
                    int rowOffset = y * stride;
                    for (int x = 1; x < width - 1; x++)
                    {
                        int idx = rowOffset + x;
                        int val = 5 * srcPixels[idx]
                                  - srcPixels[idx - 1]
                                  - srcPixels[idx + 1]
                                  - srcPixels[idx - stride]
                                  - srcPixels[idx + stride];

                        destPixels[idx] = (byte)Math.Clamp(val, 0, 255);
                    }
                }

                return BitmapSource.Create(
                    width, height,
                    source.DpiX, source.DpiY,
                    PixelFormats.Gray8,
                    null,
                    destPixels,
                    stride
                );
            }
            catch
            {
                return source;
            }
        }

        private static BitmapSource SharpenBgr24(BitmapSource source)
        {
            try
            {
                int width = source.PixelWidth;
                int height = source.PixelHeight;
                int stride = ((width * 3 + 3) / 4) * 4;
                byte[] srcPixels = new byte[height * stride];
                source.CopyPixels(srcPixels, stride, 0);

                byte[] destPixels = new byte[height * stride];
                Array.Copy(srcPixels, destPixels, srcPixels.Length);

                for (int y = 1; y < height - 1; y++)
                {
                    int rowOffset = y * stride;
                    for (int x = 1; x < width - 1; x++)
                    {
                        int idx = rowOffset + x * 3;

                        int b = 5 * srcPixels[idx]
                                - srcPixels[idx - 3]
                                - srcPixels[idx + 3]
                                - srcPixels[idx - stride]
                                - srcPixels[idx + stride];

                        int g = 5 * srcPixels[idx + 1]
                                - srcPixels[idx - 2]
                                - srcPixels[idx + 4]
                                - srcPixels[idx + 1 - stride]
                                - srcPixels[idx + 1 + stride];

                        int r = 5 * srcPixels[idx + 2]
                                - srcPixels[idx - 1]
                                - srcPixels[idx + 5]
                                - srcPixels[idx + 2 - stride]
                                - srcPixels[idx + 2 + stride];

                        destPixels[idx] = (byte)Math.Clamp(b, 0, 255);
                        destPixels[idx + 1] = (byte)Math.Clamp(g, 0, 255);
                        destPixels[idx + 2] = (byte)Math.Clamp(r, 0, 255);
                    }
                }

                return BitmapSource.Create(
                    width, height,
                    source.DpiX, source.DpiY,
                    PixelFormats.Bgr24,
                    null,
                    destPixels,
                    stride
                );
            }
            catch
            {
                return source;
            }
        }
    }
}
