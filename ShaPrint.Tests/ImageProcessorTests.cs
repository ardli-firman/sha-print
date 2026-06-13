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

            // 3. Assert
            Assert.NotNull(outputBytes);
            
            using (var ms = new MemoryStream(outputBytes))
            {
                var decoder = BitmapDecoder.Create(ms, BitmapCreateOptions.None, BitmapCacheOption.OnLoad);
                var frame = decoder.Frames[0];
                
                Assert.Equal(PixelFormats.Gray8, frame.Format); // B&W JPEG must be Gray8
                
                byte[] outputPixels = new byte[4];
                frame.CopyPixels(outputPixels, 2, 0);

                // Verify thresholding (Red -> 0, Green -> 255, Blue -> 0, Yellow -> 255)
                // Note: JPEG encoding is lossy so pixels might not be exactly 0 or 255, 
                // but they should be very close (e.g. < 5 or > 250).
                Assert.True(outputPixels[0] < 10, $"Expected pixel 0 to be near 0, got {outputPixels[0]}");
                Assert.True(outputPixels[1] > 245, $"Expected pixel 1 to be near 255, got {outputPixels[1]}");
                Assert.True(outputPixels[2] < 10, $"Expected pixel 2 to be near 0, got {outputPixels[2]}");
                Assert.True(outputPixels[3] > 245, $"Expected pixel 3 to be near 255, got {outputPixels[3]}");
            }
        }
    }
}
