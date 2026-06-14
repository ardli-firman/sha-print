using System;
using System.Threading.Tasks;

namespace ShaPrint.Server
{
    public static class ImageProcessingUtils
    {
        public static void HorizontalBoxBlur(byte[] src, byte[] dest, int width, int height, int stride, int radius)
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

        public static void VerticalBoxBlur(byte[] src, byte[] dest, int width, int height, int stride, int radius)
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

        public static void GaussianBlur3x3(byte[] src, byte[] dest, int width, int height, int stride)
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

        public static void GaussianBlur3x3Bgr24(byte[] src, byte[] dest, int width, int height, int stride)
        {
            int length = height * stride;
            Array.Copy(src, dest, length);

            Parallel.For(1, height - 1, y =>
            {
                int rowOffset = y * stride;
                int prevRowOffset = (y - 1) * stride;
                int nextRowOffset = (y + 1) * stride;

                for (int x = 1; x < width - 1; x++)
                {
                    int idx = rowOffset + x * 3;
                    int prevIdx = prevRowOffset + x * 3;
                    int nextIdx = nextRowOffset + x * 3;

                    for (int c = 0; c < 3; c++)
                    {
                        int val = (
                            src[prevIdx - 3 + c] + 2 * src[prevIdx + c] + src[prevIdx + 3 + c] +
                            2 * src[idx - 3 + c] + 4 * src[idx + c] + 2 * src[idx + 3 + c] +
                            src[nextIdx - 3 + c] + 2 * src[nextIdx + c] + src[nextIdx + 3 + c]
                        ) >> 4;

                        dest[idx + c] = (byte)val;
                    }
                }
            });
        }

        public static void NormalizeBackground(byte[] orig, byte[] bg, byte[] dest, int length)
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

        public static void ApplySoftThreshold(byte[] pixels, int length, int tMin, int tMax)
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

        public static byte[] ConvertToTransparentBgra32(byte[] grayPixels, int width, int height, int stride)
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
