using System;
using System.IO;
using System.IO.Compression;
using ShaPrint.Core;

namespace ShaPrint.WpfApp.Services.Client
{
    /// <summary>
    /// Safe zip extraction helper with containment checks and resource caps.
    /// Defends against zip-slip (path traversal) and zip bombs (entry count / size).
    /// </summary>
    internal static class SafeZipExtractor
    {
        /// <summary>
        /// Extracts a zip archive to targetDirectory with containment checks and caps.
        /// Throws on violation; caller is responsible for cleanup of targetDirectory.
        /// </summary>
        internal static void ExtractSafely(ZipArchive archive, string targetDirectory)
        {
            int entryCount = 0;
            long totalBytes = 0;

            // Normalize the target once; ensure it ends with separator for GetRelativePath.
            string normalizedTarget = Path.GetFullPath(
                targetDirectory.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar)
                + Path.DirectorySeparatorChar);

            foreach (var entry in archive.Entries)
            {
                entryCount++;
                if (entryCount > Constants.DriverExtractMaxEntryCount)
                    throw new InvalidOperationException(
                        $"Driver package rejected: too many entries ({entryCount} > {Constants.DriverExtractMaxEntryCount}).");

                // Name length check
                if (entry.FullName.Length > Constants.DriverExtractMaxEntryNameLength)
                    throw new InvalidOperationException(
                        $"Driver package rejected: entry name too long ({entry.FullName.Length} > {Constants.DriverExtractMaxEntryNameLength}).");

                // Absolute / rooted path check
                if (Path.IsPathRooted(entry.FullName) || entry.FullName.StartsWith('/') || entry.FullName.StartsWith('\\'))
                    throw new InvalidOperationException(
                        $"Driver package rejected: entry '{entry.FullName}' is an absolute path (zip-slip).");

                // Containment check — resolve the destination path and verify it stays
                // under targetDirectory. Uses Path.GetRelativePath to avoid prefix-collision
                // false positives (review-mandated: NOT plain StartsWith).
                string destinationPath = Path.GetFullPath(Path.Combine(targetDirectory, entry.FullName));
                string relative = Path.GetRelativePath(normalizedTarget, destinationPath);
                if (relative.StartsWith("..", StringComparison.Ordinal) || Path.IsPathRooted(relative))
                    throw new InvalidOperationException(
                        $"Driver package rejected: entry '{entry.FullName}' escapes target directory (zip-slip).");

                // Size accumulation (skip directory entries)
                if (!entry.FullName.EndsWith("/"))
                {
                    totalBytes += entry.Length;
                    if (totalBytes > Constants.DriverExtractMaxTotalBytes)
                        throw new InvalidOperationException(
                            $"Driver package rejected: extracted size exceeds {Constants.DriverExtractMaxTotalBytes / (1024 * 1024 * 1024)} GiB limit.");

                    // Extract the entry
                    Directory.CreateDirectory(Path.GetDirectoryName(destinationPath)!);
                    using var entryStream = entry.Open();
                    using var fileStream = File.Create(destinationPath);
                    entryStream.CopyTo(fileStream);
                }
                else
                {
                    Directory.CreateDirectory(destinationPath);
                }
            }
        }
    }
}
