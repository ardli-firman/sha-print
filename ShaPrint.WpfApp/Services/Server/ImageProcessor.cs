using System;
using System.Buffers;
using System.IO;
using System.Threading.Tasks;
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

                        int width = processedSource.PixelWidth;
                        int height = processedSource.PixelHeight;
                        int stride = width;
                        int length = height * stride;

                        // Rent buffers from ArrayPool to avoid LOH allocations
                        byte[] origBuffer = ArrayPool<byte>.Shared.Rent(length);
                        byte[] tempBuffer = ArrayPool<byte>.Shared.Rent(length);
                        byte[] bgBuffer = ArrayPool<byte>.Shared.Rent(length);

                        try
                        {
                            processedSource.CopyPixels(origBuffer, stride, 0);

                            // Calculate dynamic radius based on maximum image dimension (1.5% of max dimension)
                            int maxDim = Math.Max(width, height);
                            int radius = Math.Max(15, (int)(maxDim * 0.015));

                            // 1. Background Normalization (Shadow Division)
                            // Horizontal box blur: orig -> temp
                            HorizontalBoxBlur(origBuffer, tempBuffer, width, height, stride, radius);
                            // Vertical box blur: temp -> bg
                            VerticalBoxBlur(tempBuffer, bgBuffer, width, height, stride, radius);

                            // Divide: orig = (orig * 255) / bg
                            NormalizeBackground(origBuffer, bgBuffer, origBuffer, length);

                            // 2. Gaussian Smoothing (3x3) to remove high-frequency scanner noise: orig -> temp
                            GaussianBlur3x3(origBuffer, tempBuffer, width, height, stride);

                            // 3. Soft Thresholding in-place on temp: range [180, 240] for post-normalized values
                            ApplySoftThreshold(tempBuffer, length, 180, 240);

                            if (isTargetPng)
                            {
                                // Convert Gray8 to transparent Bgra32
                                byte[] bgraPixels = ConvertToTransparentBgra32(tempBuffer, width, height, stride);
                                int bgraStride = width * 4;

                                var transparentSource = BitmapSource.Create(
                                    width, height,
                                    processedSource.DpiX, processedSource.DpiY,
                                    PixelFormats.Bgra32,
                                    null,
                                    bgraPixels,
                                    bgraStride
                                );
                                processedSource = transparentSource;
                            }
                            else
                            {
                                // Return as Gray8
                                byte[] finalPixels = new byte[length];
                                Array.Copy(tempBuffer, finalPixels, length);

                                var bwSource = BitmapSource.Create(
                                    width, height,
                                    processedSource.DpiX, processedSource.DpiY,
                                    PixelFormats.Gray8,
                                    null,
                                    finalPixels,
                                    stride
                                );
                                processedSource = bwSource;
                            }
                        }
                        finally
                        {
                            // Return rented buffers
                            ArrayPool<byte>.Shared.Return(origBuffer);
                            ArrayPool<byte>.Shared.Return(tempBuffer);
                            ArrayPool<byte>.Shared.Return(bgBuffer);
                        }
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

        private static void HorizontalBoxBlur(byte[] src, byte[] dest, int width, int height, int stride, int radius)
        {
            int windowSize = 2 * radius + 1;
            Parallel.For(0, height, y =>
            {
                int rowOffset = y * stride;
                int sum = 0;

                // Initialize the first window
                for (int x = -radius; x <= radius; x++)
                {
                    int clampX = Math.Clamp(x, 0, width - 1);
                    sum += src[rowOffset + clampX];
                }
                dest[rowOffset + 0] = (byte)(sum / windowSize);

                for (int x = 1; x < width; x++)
                {
                    int prevX = Math.Clamp(x - radius - 1, 0, width - 1);
                    int nextX = Math.Clamp(x + radius, 0, width - 1);
                    sum = sum - src[rowOffset + prevX] + src[rowOffset + nextX];
                    dest[rowOffset + x] = (byte)(sum / windowSize);
                }
            });
        }

        private static void VerticalBoxBlur(byte[] src, byte[] dest, int width, int height, int stride, int radius)
        {
            int windowSize = 2 * radius + 1;
            Parallel.For(0, width, x =>
            {
                int sum = 0;

                // Initialize the first window
                for (int y = -radius; y <= radius; y++)
                {
                    int clampY = Math.Clamp(y, 0, height - 1);
                    sum += src[clampY * stride + x];
                }
                dest[0 * stride + x] = (byte)(sum / windowSize);

                for (int y = 1; y < height; y++)
                {
                    int prevY = Math.Clamp(y - radius - 1, 0, height - 1);
                    int nextY = Math.Clamp(y + radius, 0, height - 1);
                    sum = sum - src[prevY * stride + x] + src[nextY * stride + x];
                    dest[y * stride + x] = (byte)(sum / windowSize);
                }
            });
        }

        private static void GaussianBlur3x3(byte[] src, byte[] dest, int width, int height, int stride)
        {
            // Copy borders to dest first, since 3x3 cannot process borders
            for (int x = 0; x < width; x++)
            {
                dest[x] = src[x];
                dest[(height - 1) * stride + x] = src[(height - 1) * stride + x];
            }
            for (int y = 1; y < height - 1; y++)
            {
                dest[y * stride] = src[y * stride];
                dest[y * stride + width - 1] = src[y * stride + width - 1];
            }

            // Process interior
            Parallel.For(1, height - 1, y =>
            {
                int rowOffset = y * stride;
                int prevRowOffset = (y - 1) * stride;
                int nextRowOffset = (y + 1) * stride;

                for (int x = 1; x < width - 1; x++)
                {
                    int val = (
                        src[prevRowOffset + x - 1] + 2 * src[prevRowOffset + x] + src[prevRowOffset + x + 1] +
                        2 * src[rowOffset + x - 1] + 4 * src[rowOffset + x] + 2 * src[rowOffset + x + 1] +
                        src[nextRowOffset + x - 1] + 2 * src[nextRowOffset + x] + src[nextRowOffset + x + 1]
                    ) >> 4;

                    dest[rowOffset + x] = (byte)val;
                }
            });
        }

        private static void NormalizeBackground(byte[] orig, byte[] bg, byte[] dest, int length)
        {
            Parallel.For(0, length, i =>
            {
                int bgVal = bg[i];
                int origVal = orig[i];

                if (bgVal == 0)
                {
                    dest[i] = 0;
                }
                else
                {
                    int val = (origVal * 255) / bgVal;
                    dest[i] = (byte)Math.Clamp(val, 0, 255);
                }
            });
        }

        private static void ApplySoftThreshold(byte[] pixels, int length, int tMin, int tMax)
        {
            int divisor = tMax - tMin;
            if (divisor <= 0) divisor = 1;

            Parallel.For(0, length, i =>
            {
                int val = pixels[i];
                if (val < tMin)
                    pixels[i] = 0;
                else if (val > tMax)
                    pixels[i] = 255;
                else
                    pixels[i] = (byte)((val - tMin) * 255 / divisor);
            });
        }

        private static byte[] ConvertToTransparentBgra32(byte[] grayPixels, int width, int height, int stride)
        {
            int bgraStride = width * 4;
            byte[] bgraPixels = new byte[height * bgraStride];

            Parallel.For(0, height, y =>
            {
                int grayOffset = y * stride;
                int bgraOffset = y * bgraStride;

                for (int x = 0; x < width; x++)
                {
                    byte g = grayPixels[grayOffset + x];
                    int idx = bgraOffset + x * 4;

                    bgraPixels[idx] = 0;     // B
                    bgraPixels[idx + 1] = 0; // G
                    bgraPixels[idx + 2] = 0; // R
                    bgraPixels[idx + 3] = (byte)(255 - g); // A
                }
            });

            return bgraPixels;
        }
    }
}
