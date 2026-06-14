using System;
using System.Buffers;
using System.Threading.Tasks;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace ShaPrint.Server
{
    public static class GrayscaleImageProcessor
    {
        public static BitmapSource Process(BitmapSource source)
        {
            // Grayscale: convert to Gray8
            if (source.Format != PixelFormats.Gray8)
            {
                source = new FormatConvertedBitmap(source, PixelFormats.Gray8, null, 0);
            }

            int width = source.PixelWidth;
            int height = source.PixelHeight;
            int stride = width;
            int length = height * stride;

            byte[] origBuffer = ArrayPool<byte>.Shared.Rent(length);
            byte[] blurredBuffer = ArrayPool<byte>.Shared.Rent(length);
            byte[] tempBuffer = ArrayPool<byte>.Shared.Rent(length);

            try
            {
                source.CopyPixels(origBuffer, stride, 0);

                // 1. Gaussian Blur (3x3): origBuffer -> blurredBuffer
                ImageProcessingUtils.GaussianBlur3x3(origBuffer, blurredBuffer, width, height, stride);

                // 2. Unsharp Masking: origBuffer + 1.5 * (origBuffer - blurredBuffer) -> tempBuffer
                Array.Copy(origBuffer, tempBuffer, length);

                Parallel.For(1, height - 1, y =>
                {
                    int rowOffset = y * stride;
                    for (int x = 1; x < width - 1; x++)
                    {
                        int idx = rowOffset + x;
                        int origVal = origBuffer[idx];
                        int blurVal = blurredBuffer[idx];
                        int diff = origVal - blurVal;
                        int sharpened = origVal + (3 * diff) / 2;
                        tempBuffer[idx] = (byte)Math.Clamp(sharpened, 0, 255);
                    }
                });

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
            finally
            {
                ArrayPool<byte>.Shared.Return(origBuffer);
                ArrayPool<byte>.Shared.Return(blurredBuffer);
                ArrayPool<byte>.Shared.Return(tempBuffer);
            }
        }
    }
}
