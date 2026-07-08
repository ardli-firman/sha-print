using System.IO;
using ShaPrint.WpfApp.Utils;
using Xunit;

namespace ShaPrint.Tests
{
    public class StartupManagerTests
    {
        [Fact]
        public void GenerateXml_StandardPath_GeneratesCorrectXml()
        {
            // Arrange
            string exePath = @"C:\ProgramFiles\ShaPrint\ShaPrint.exe";

            // Act
            string xml = StartupManager.GenerateXml(exePath);

            // Assert
            Assert.Contains("<GroupId>S-1-5-32-545</GroupId>", xml);
            Assert.DoesNotContain("<UserId>", xml);
            Assert.Contains("<Command>&quot;C:\\ProgramFiles\\ShaPrint\\ShaPrint.exe&quot;</Command>", xml);
            Assert.Contains("<WorkingDirectory>C:\\ProgramFiles\\ShaPrint</WorkingDirectory>", xml);
            Assert.Contains("<Arguments>--startup</Arguments>", xml);
        }

        [Fact]
        public void GenerateXml_PathWithSpaces_EscapesAndQuotesCorrectly()
        {
            // Arrange
            string exePath = @"C:\Program Files\Sha Print\ShaPrint.exe";

            // Act
            string xml = StartupManager.GenerateXml(exePath);

            // Assert
            Assert.Contains("<GroupId>S-1-5-32-545</GroupId>", xml);
            Assert.Contains("<Command>&quot;C:\\Program Files\\Sha Print\\ShaPrint.exe&quot;</Command>", xml);
            Assert.Contains("<WorkingDirectory>C:\\Program Files\\Sha Print</WorkingDirectory>", xml);
        }

        [Fact]
        public void GenerateXml_PathWithXmlSpecialCharacters_EscapesCorrectly()
        {
            // Arrange
            string exePath = @"C:\Sha & Print\Sha<Print>.exe";

            // Act
            string xml = StartupManager.GenerateXml(exePath);

            // Assert
            // & -> &amp;, < -> &lt;, > -> &gt;
            Assert.Contains("<Command>&quot;C:\\Sha &amp; Print\\Sha&lt;Print&gt;.exe&quot;</Command>", xml);
            Assert.Contains("<WorkingDirectory>C:\\Sha &amp; Print</WorkingDirectory>", xml);
        }

        [Fact]
        public void GenerateXml_RootDirectoryPath_HandlesWorkingDirectoryCorrectly()
        {
            // Arrange
            string exePath = @"C:\ShaPrint.exe";

            // Act
            string xml = StartupManager.GenerateXml(exePath);

            // Assert
            Assert.Contains("<Command>&quot;C:\\ShaPrint.exe&quot;</Command>", xml);
            Assert.Contains("<WorkingDirectory>C:\\</WorkingDirectory>", xml);
        }
    }
}
