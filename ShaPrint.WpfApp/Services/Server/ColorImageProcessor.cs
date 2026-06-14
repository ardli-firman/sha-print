using System;
using System.Buffers;
using System.Threading.Tasks;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace ShaPrint.Server
{
    public static class ColorImageProcessor
    {
        public static BitmapSource Process(BitmapSource source)
        {
            // Color: convert to Bgr24
            if (source.Format != PixelFormats.Bgr24)
            {
                source = new FormatConvertedBitmap(source, PixelFormats.Bgr24, null, 0);
            }

            int width = source.PixelWidth;
            int height = source.PixelHeight;
            int strideBgr = ((width * 3 + 3) / 4) * 4;
            int lengthBgr = height * strideBgr;

            byte[] origBgr = ArrayPool<byte>.Shared.Rent(lengthBgr);
            byte[] blurredBgr = ArrayPool<byte>.Shared.Rent(lengthBgr);
            byte[] tempBgr = ArrayPool<byte>.Shared.Rent(lengthBgr);

            try
            {
                source.CopyPixels(origBgr, strideBgr, 0);

                // 1. Gaussian Blur (3x3) Bgr24: origBgr -> blurredBgr
                ImageProcessingUtils.GaussianBlur3x3Bgr24(origBgr, blurredBgr, width, height, strideBgr);

                // 2. Unsharp Masking: origBgr + 1.5 * (origBgr - blurredBgr) -> tempBgr
                Array.Copy(origBgr, tempBgr, lengthBgr);

                Parallel.For(1, height - 1, y =>
                {
                    int rowOffset = y * strideBgr;
                    for (int x = 1; x < width - 1; x++)
                    {
                        int idx = rowOffset + x * 3;
                        for (int c = 0; c < 3; c++)
                        {
                            int origVal = origBgr[idx + c];
                            int blurVal = blurredBgr[idx + c];
                            int diff = origVal - blurVal;
                            int sharpened = origVal + (3 * diff) / 2;
                            tempBgr[idx + c] = (byte)Math.Clamp(sharpened, 0, 255);
                        }
                    }
                });

                byte[] finalPixels = new byte[lengthBgr];
                Array.Copy(tempBgr, finalPixels, lengthBgr);

                return BitmapSource.Create(
                    width, height,
                    source.DpiX, source.DpiY,
                    PixelFormats.Bgr24,
                    null,
                    finalPixels,
                    strideBgr
                );
            }
            finally
            {
                ArrayPool<byte>.Shared.Return(origBgr);
                ArrayPool<byte>.Shared.Return(blurredBgr);
                ArrayPool<byte>.Shared.Return(tempBgr);
            }
        }
    }
}
