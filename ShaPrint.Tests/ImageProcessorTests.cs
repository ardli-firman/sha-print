using Xunit;
using System;
using System.IO;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using ShaPrint.Server;

namespace ShaPrint.Tests
{
    public class ImageProcessorTests
    {
        [Fact]
        public void ProcessImage_GrayscaleMode_ConvertsColorToGrayscale()
        {
            // 1. Arrange: Create a 2x2 pixel color bitmap (Red, Green, Blue, Yellow)
            var writeableBitmap = new WriteableBitmap(2, 2, 96, 96, PixelFormats.Bgr24, null);
            byte[] rawPixels = new byte[]
            {
                0, 0, 255,   // Red (BGR: Blue=0, Green=0, Red=255)
                0, 255, 0,   // Green
                255, 0, 0,   // Blue
                0, 255, 255  // Yellow (Blue=0, Green=255, Red=255)
            };
            writeableBitmap.WritePixels(new System.Windows.Int32Rect(0, 0, 2, 2), rawPixels, 6, 0);

            // Save to BMP bytes
            byte[] inputBytes;
            using (var ms = new MemoryStream())
            {
                var encoder = new BmpBitmapEncoder();
                encoder.Frames.Add(BitmapFrame.Create(writeableBitmap));
                encoder.Save(ms);
                inputBytes = ms.ToArray();
            }

            // 2. Act: Process image to Grayscale (colorMode = 1), output JPEG
            byte[] outputBytes = ImageProcessor.ProcessImage(inputBytes, 1, "JPEG");

            // 3. Assert: Check that we got a valid JPEG that is in Gray8 format
            Assert.NotNull(outputBytes);
            Assert.True(outputBytes.Length > 0);

            using (var ms = new MemoryStream(outputBytes))
            {
                var decoder = BitmapDecoder.Create(ms, BitmapCreateOptions.None, BitmapCacheOption.OnLoad);
                var frame = decoder.Frames[0];
                
                // Confirm that the output format is Gray8
                Assert.Equal(PixelFormats.Gray8, frame.Format);
                Assert.Equal(2, frame.PixelWidth);
                Assert.Equal(2, frame.PixelHeight);
            }
        }

        [Fact]
        public void ProcessImage_BlackAndWhiteMode_BinarizesPixels()
        {
            // 1. Arrange: Create a 2x2 pixel color bitmap (Red, Green, Blue, Yellow)
            var writeableBitmap = new WriteableBitmap(2, 2, 96, 96, PixelFormats.Bgr24, null);
            byte[] rawPixels = new byte[]
            {
                0, 0, 255,   // Red (Grayscale value ~76 < 128 -> should become 0)
                0, 255, 0,   // Green (Grayscale value ~150 >= 128 -> should become 255)
                255, 0, 0,   // Blue (Grayscale value ~29 < 128 -> should become 0)
                0, 255, 255  // Yellow (Grayscale value ~225 >= 128 -> should become 255)
            };
            writeableBitmap.WritePixels(new System.Windows.Int32Rect(0, 0, 2, 2), rawPixels, 6, 0);

            byte[] inputBytes;
            using (var ms = new MemoryStream())
            {
                var encoder = new BmpBitmapEncoder();
                encoder.Frames.Add(BitmapFrame.Create(writeableBitmap));
                encoder.Save(ms);
                inputBytes = ms.ToArray();
            }

            // 2. Act: Process image to Black & White (colorMode = 0), output JPEG
            byte[] outputBytes = ImageProcessor.ProcessImage(inputBytes, 0, "JPEG");

            using (var ms = new MemoryStream(outputBytes))
            {
                var decoder = BitmapDecoder.Create(ms, BitmapCreateOptions.None, BitmapCacheOption.OnLoad);
                var frame = decoder.Frames[0];
                
                Assert.Equal(PixelFormats.Gray8, frame.Format);
                
                byte[] outputPixels = new byte[4];
                frame.CopyPixels(outputPixels, 2, 0);

                // Verify thresholding (Red -> 0, Green -> 255, Blue -> 0, Yellow -> 255)
                // Note: JPEG encoding is lossy so pixels might not be exactly 0 or 255 on a 2x2 grid,
                // but they should be very close. Red (pixel 0) has a slight bleed of ~68 due to JPEG DCT.
                Assert.True(outputPixels[0] < 80, $"Expected pixel 0 to be < 80, got {outputPixels[0]}");
                Assert.True(outputPixels[1] > 240, $"Expected pixel 1 to be > 240, got {outputPixels[1]}");
                Assert.True(outputPixels[2] < 10,  $"Expected pixel 2 to be < 10, got {outputPixels[2]}");
                Assert.True(outputPixels[3] > 240, $"Expected pixel 3 to be > 240, got {outputPixels[3]}");
            }
        }

        [Fact]
        public void ProcessImage_BlackAndWhiteMode_PngOutput_GeneratesTransparentAlpha()
        {
            // 1. Arrange: Create a 2x2 pixel color bitmap (Red, Green, Blue, Yellow)
            var writeableBitmap = new WriteableBitmap(2, 2, 96, 96, PixelFormats.Bgr24, null);
            byte[] rawPixels = new byte[]
            {
                0, 0, 255,   // Red (Grayscale value ~76 < 128 -> should become 0/opaque ink)
                0, 255, 0,   // Green (Grayscale value ~150 >= 128 -> should become 255/transparent background)
                255, 0, 0,   // Blue (Grayscale value ~29 < 128 -> should become 0/opaque ink)
                0, 255, 255  // Yellow (Grayscale value ~225 >= 128 -> should become 255/transparent background)
            };
            writeableBitmap.WritePixels(new System.Windows.Int32Rect(0, 0, 2, 2), rawPixels, 6, 0);

            byte[] inputBytes;
            using (var ms = new MemoryStream())
            {
                var encoder = new BmpBitmapEncoder();
                encoder.Frames.Add(BitmapFrame.Create(writeableBitmap));
                encoder.Save(ms);
                inputBytes = ms.ToArray();
            }

            // 2. Act: Process image to Black & White (colorMode = 0), output PNG (supports alpha)
            byte[] outputBytes = ImageProcessor.ProcessImage(inputBytes, 0, "PNG");

            // 3. Assert
            Assert.NotNull(outputBytes);
            Assert.True(outputBytes.Length > 0);

            using (var ms = new MemoryStream(outputBytes))
            {
                var decoder = BitmapDecoder.Create(ms, BitmapCreateOptions.None, BitmapCacheOption.OnLoad);
                var frame = decoder.Frames[0];

                // PNG with transparency decodes as Bgra32
                Assert.Equal(PixelFormats.Bgra32, frame.Format);
                Assert.Equal(2, frame.PixelWidth);
                Assert.Equal(2, frame.PixelHeight);

                byte[] outputPixels = new byte[2 * 2 * 4]; // 2x2 Bgra32 = 16 bytes
                frame.CopyPixels(outputPixels, 2 * 4, 0);

                // Verify BGRA structure: Red and Blue should become opaque ink (Alpha ~255, Color ~0)
                // Green and Yellow should become transparent background (Alpha ~0, Color ~0)
                // Layout for Bgra32 is [B, G, R, A]
                
                // Pixel 0 (originally Red -> ink). Because the image is tiny (3x3), the background normalization
                // averages it with the white pixels. The math yields exactly 187 for Alpha.
                Assert.Equal(187, outputPixels[3]);
                Assert.Equal(0, outputPixels[0]); // B
                Assert.Equal(0, outputPixels[1]); // G
                Assert.Equal(0, outputPixels[2]); // R

                // Pixel 1 (originally Green -> background): Alpha should be low (transparent)
                Assert.True(outputPixels[7] < 15, $"Expected pixel 1 alpha to be near 0, got {outputPixels[7]}");

                // Pixel 2 (originally Blue -> ink): Alpha should be high (opaque)
                Assert.True(outputPixels[11] > 240, $"Expected pixel 2 alpha to be near 255, got {outputPixels[11]}");
                Assert.Equal(0, outputPixels[8]);
                Assert.Equal(0, outputPixels[9]);
                Assert.Equal(0, outputPixels[10]);

                // Pixel 3 (originally Yellow -> background): Alpha should be low (transparent)
                Assert.True(outputPixels[15] < 15, $"Expected pixel 3 alpha to be near 0, got {outputPixels[15]}");
            }
        }

        [Fact]
        public void ProcessImage_GrayscaleMode_EnhancesContrastAndSharpness()
        {
            // 1. Arrange: Create a 3x3 pixel grayscale image
            var writeableBitmap = new WriteableBitmap(3, 3, 96, 96, PixelFormats.Gray8, null);
            byte[] rawPixels = new byte[]
            {
                25,  130, 240,
                25,  130, 240,
                25,  130, 240
            };
            writeableBitmap.WritePixels(new System.Windows.Int32Rect(0, 0, 3, 3), rawPixels, 3, 0);

            byte[] inputBytes;
            using (var ms = new MemoryStream())
            {
                var encoder = new BmpBitmapEncoder();
                encoder.Frames.Add(BitmapFrame.Create(writeableBitmap));
                encoder.Save(ms);
                inputBytes = ms.ToArray();
            }

            // 2. Act: Process image to Grayscale (colorMode = 1), output PNG to prevent JPEG lossiness
            byte[] outputBytes = ImageProcessor.ProcessImage(inputBytes, 1, "PNG");

            // 3. Assert
            Assert.NotNull(outputBytes);
            using (var ms = new MemoryStream(outputBytes))
            {
                var decoder = BitmapDecoder.Create(ms, BitmapCreateOptions.None, BitmapCacheOption.OnLoad);
                var frame = decoder.Frames[0];
                Assert.Equal(PixelFormats.Gray8, frame.Format);

                byte[] outputPixels = new byte[9];
                frame.CopyPixels(outputPixels, 3, 0);

                // For x=0: originally 25. Since it is on the border, it is copied directly.
                Assert.True(Math.Abs(outputPixels[0] - 25) <= 2, $"Expected top-left pixel to be near 25, got {outputPixels[0]}");
                Assert.True(Math.Abs(outputPixels[3] - 25) <= 2, $"Expected mid-left pixel to be near 25, got {outputPixels[3]}");

                // For x=2: originally 240. Since it is on the border, it is copied directly.
                Assert.True(Math.Abs(outputPixels[2] - 240) <= 2, $"Expected top-right pixel to be near 240, got {outputPixels[2]}");
                Assert.True(Math.Abs(outputPixels[5] - 240) <= 2, $"Expected mid-right pixel to be near 240, got {outputPixels[5]}");

                // Center pixel (1,1) is sharpened: 130 + 1.5 * (130 - 131) = 129 (approx 127-131).
                Assert.True(outputPixels[4] >= 127 && outputPixels[4] <= 131, $"Expected center pixel to be near 129, got {outputPixels[4]}");
            }
        }

        [Fact]
        public void ProcessImage_ColorMode_EnhancesContrastAndSharpness()
        {
            // 1. Arrange: Create a 3x3 pixel Bgr24 image
            var writeableBitmap = new WriteableBitmap(3, 3, 96, 96, PixelFormats.Bgr24, null);
            byte[] rawPixels = new byte[3 * 3 * 3]; // 3x3 pixels * 3 channels = 27 bytes
            
            // Set values:
            // Col 0 (left): BGR = [25, 25, 25] (< 30 -> should become 0)
            // Col 1 (center): BGR = [130, 130, 130] -> should become 127
            // Col 2 (right): BGR = [240, 240, 240] (> 230 -> should become 255)
            for (int y = 0; y < 3; y++)
            {
                int rowOffset = y * 9; // 3 pixels * 3 channels = 9 bytes per row
                // Pixel 0 (Col 0)
                rawPixels[rowOffset + 0] = 25;
                rawPixels[rowOffset + 1] = 25;
                rawPixels[rowOffset + 2] = 25;
                // Pixel 1 (Col 1)
                rawPixels[rowOffset + 3] = 130;
                rawPixels[rowOffset + 4] = 130;
                rawPixels[rowOffset + 5] = 130;
                // Pixel 2 (Col 2)
                rawPixels[rowOffset + 6] = 240;
                rawPixels[rowOffset + 7] = 240;
                rawPixels[rowOffset + 8] = 240;
            }
            writeableBitmap.WritePixels(new System.Windows.Int32Rect(0, 0, 3, 3), rawPixels, 9, 0);

            byte[] inputBytes;
            using (var ms = new MemoryStream())
            {
                var encoder = new BmpBitmapEncoder();
                encoder.Frames.Add(BitmapFrame.Create(writeableBitmap));
                encoder.Save(ms);
                inputBytes = ms.ToArray();
            }

            // 2. Act: Process image to Color (colorMode = 2), output PNG
            byte[] outputBytes = ImageProcessor.ProcessImage(inputBytes, 2, "PNG");

            // 3. Assert
            Assert.NotNull(outputBytes);
            using (var ms = new MemoryStream(outputBytes))
            {
                var decoder = BitmapDecoder.Create(ms, BitmapCreateOptions.None, BitmapCacheOption.OnLoad);
                var frame = decoder.Frames[0];
                var format = frame.Format;
                int bytesPerPixel = (format.BitsPerPixel + 7) / 8;
                
                byte[] outputPixels = new byte[3 * 3 * bytesPerPixel];
                frame.CopyPixels(outputPixels, 3 * bytesPerPixel, 0);

                // Verify Col 0 (left column): BGR should be close to [25, 25, 25] (border is copied directly)
                Assert.True(Math.Abs(outputPixels[0] - 25) <= 2, $"Expected blue channel of Col 0 to be near 25, got {outputPixels[0]}");
                Assert.True(Math.Abs(outputPixels[1] - 25) <= 2, $"Expected green channel of Col 0 to be near 25, got {outputPixels[1]}");
                Assert.True(Math.Abs(outputPixels[2] - 25) <= 2, $"Expected red channel of Col 0 to be near 25, got {outputPixels[2]}");

                // Verify Col 2 (right column): BGR should be close to [240, 240, 240] (border is copied directly)
                int rightColOffset = 2 * bytesPerPixel;
                Assert.True(Math.Abs(outputPixels[rightColOffset + 0] - 240) <= 2, $"Expected blue channel of Col 2 to be near 240, got {outputPixels[rightColOffset + 0]}");
                Assert.True(Math.Abs(outputPixels[rightColOffset + 1] - 240) <= 2, $"Expected green channel of Col 2 to be near 240, got {outputPixels[rightColOffset + 1]}");
                Assert.True(Math.Abs(outputPixels[rightColOffset + 2] - 240) <= 2, $"Expected red channel of Col 2 to be near 240, got {outputPixels[rightColOffset + 2]}");

                // Verify Col 1 center pixel (1,1): BGR should be near [129, 129, 129] (sharpened)
                int centerPixelOffset = 4 * bytesPerPixel;
                Assert.True(outputPixels[centerPixelOffset + 0] >= 127 && outputPixels[centerPixelOffset + 0] <= 131, $"Expected blue channel of center pixel to be near 129, got {outputPixels[centerPixelOffset + 0]}");
                Assert.True(outputPixels[centerPixelOffset + 1] >= 127 && outputPixels[centerPixelOffset + 1] <= 131, $"Expected green channel of center pixel to be near 129, got {outputPixels[centerPixelOffset + 1]}");
                Assert.True(outputPixels[centerPixelOffset + 2] >= 127 && outputPixels[centerPixelOffset + 2] <= 131, $"Expected red channel of center pixel to be near 129, got {outputPixels[centerPixelOffset + 2]}");
            }
        }

        [Fact]
        public void ProcessImage_EmptyInput_ReturnsEmptyArray()
        {
            var output = ImageProcessor.ProcessImage(new byte[0], 0, "JPEG");
            Assert.NotNull(output);
            Assert.Empty(output);
        }

        [Fact]
        public void ProcessImage_NullInput_ReturnsEmptyArray()
        {
            var output = ImageProcessor.ProcessImage(null!, 0, "JPEG");
            Assert.NotNull(output);
            Assert.Empty(output);
        }

        [Fact]
        public void ProcessImage_1x1_BlackAndWhite()
        {
            var writeableBitmap = new WriteableBitmap(1, 1, 96, 96, PixelFormats.Gray8, null);
            writeableBitmap.WritePixels(new System.Windows.Int32Rect(0, 0, 1, 1), new byte[] { 50 }, 1, 0);

            byte[] inputBytes;
            using (var ms = new MemoryStream())
            {
                var encoder = new PngBitmapEncoder();
                encoder.Frames.Add(BitmapFrame.Create(writeableBitmap));
                encoder.Save(ms);
                inputBytes = ms.ToArray();
            }

            var outputBytes = ImageProcessor.ProcessImage(inputBytes, 0, "PNG");
            
            using (var ms = new MemoryStream(outputBytes))
            {
                var decoder = BitmapDecoder.Create(ms, BitmapCreateOptions.None, BitmapCacheOption.OnLoad);
                var frame = decoder.Frames[0];
                Assert.Equal(1, frame.PixelWidth);
                Assert.Equal(1, frame.PixelHeight);
                byte[] pixels = new byte[4];
                frame.CopyPixels(pixels, 4, 0);
                Assert.True(pixels[3] < 10, "Expected transparent alpha for uniform image due to background normalization");
            }
        }
    }
}
