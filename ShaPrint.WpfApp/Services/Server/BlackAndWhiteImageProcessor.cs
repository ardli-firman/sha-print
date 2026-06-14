using System;
using System.Buffers;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace ShaPrint.Server
{
    public static class BlackAndWhiteImageProcessor
    {
        public static BitmapSource Process(BitmapSource source, bool isTargetPng)
        {
            // B&W: convert to Gray8
            if (source.Format != PixelFormats.Gray8)
            {
                source = new FormatConvertedBitmap(source, PixelFormats.Gray8, null, 0);
            }

            int width = source.PixelWidth;
            int height = source.PixelHeight;
            int stride = width;
            int length = height * stride;

            // Rent buffers from ArrayPool to avoid LOH allocations
            byte[] origBuffer = ArrayPool<byte>.Shared.Rent(length);
            byte[] tempBuffer = ArrayPool<byte>.Shared.Rent(length);
            byte[] bgBuffer = ArrayPool<byte>.Shared.Rent(length);

            try
            {
                source.CopyPixels(origBuffer, stride, 0);

                // Calculate dynamic radius based on maximum image dimension (1.5% of max dimension)
                int maxDim = Math.Max(width, height);
                int radius = Math.Max(15, (int)(maxDim * 0.015));

                // 1. Background Normalization (Shadow Division)
                ImageProcessingUtils.HorizontalBoxBlur(origBuffer, tempBuffer, width, height, stride, radius);
                ImageProcessingUtils.VerticalBoxBlur(tempBuffer, bgBuffer, width, height, stride, radius);

                // Divide: orig = (orig * 255) / bg
                ImageProcessingUtils.NormalizeBackground(origBuffer, bgBuffer, origBuffer, length);

                // 2. Gaussian Smoothing (3x3) to remove high-frequency scanner noise
                ImageProcessingUtils.GaussianBlur3x3(origBuffer, tempBuffer, width, height, stride);

                // 3. Soft Thresholding in-place on temp: range [180, 240] for post-normalized values
                ImageProcessingUtils.ApplySoftThreshold(tempBuffer, length, 180, 240);

                if (isTargetPng)
                {
                    // Convert Gray8 to transparent Bgra32
                    byte[] bgraPixels = ImageProcessingUtils.ConvertToTransparentBgra32(tempBuffer, width, height, stride);
                    int bgraStride = width * 4;

                    return BitmapSource.Create(
                        width, height,
                        source.DpiX, source.DpiY,
                        PixelFormats.Bgra32,
                        null,
                        bgraPixels,
                        bgraStride
                    );
                }
                else
                {
                    // Return as Gray8
                    byte[] finalPixels = new byte[length];
                    Array.Copy(tempBuffer, finalPixels, length);

                    return BitmapSource.Create(
                        width, height,
                        source.DpiX, source.DpiY,
                        PixelFormats.Gray8,
                        null,
                        finalPixels,
                        stride
                    );
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
    }
}
