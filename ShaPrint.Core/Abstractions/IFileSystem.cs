using System.IO;
using System.Threading.Tasks;

namespace ShaPrint.Core.Abstractions
{
    /// <summary>
    /// Abstraction over filesystem operations for testability.
    /// </summary>
    public interface IFileSystem
    {
        Task WriteAllBytesAsync(string path, byte[] data);
        Task<byte[]> ReadAllBytesAsync(string path);
        bool FileExists(string path);
        bool DirectoryExists(string path);
        void CreateDirectory(string path);
        long GetFileSize(string path);
        void DeleteFile(string path);
        void DeleteDirectory(string path, bool recursive);
        string[] GetFiles(string path, string searchPattern, SearchOption searchOption);
    }

    /// <summary>
    /// Optional streaming seam for large artifacts. Keeping this separate from
    /// IFileSystem preserves existing fakes while production avoids allocating an
    /// entire driver archive for each client.
    /// </summary>
    public interface IStreamingFileSystem
    {
        Stream OpenRead(string path);
    }

    /// <summary>
    /// Default implementation using System.IO.
    /// </summary>
    public class RealFileSystem : IFileSystem, IStreamingFileSystem
    {
        public async Task WriteAllBytesAsync(string path, byte[] data)
        {
            var dir = Path.GetDirectoryName(path);
            if (dir != null && !Directory.Exists(dir))
                Directory.CreateDirectory(dir);
            await File.WriteAllBytesAsync(path, data);
        }

        public async Task<byte[]> ReadAllBytesAsync(string path)
            => await File.ReadAllBytesAsync(path);

        public Stream OpenRead(string path) => new FileStream(
            path,
            FileMode.Open,
            FileAccess.Read,
            FileShare.Read,
            bufferSize: 64 * 1024,
            options: FileOptions.Asynchronous | FileOptions.SequentialScan);

        public bool FileExists(string path) => File.Exists(path);

        public bool DirectoryExists(string path) => Directory.Exists(path);

        public void CreateDirectory(string path) => Directory.CreateDirectory(path);

        public long GetFileSize(string path) => new FileInfo(path).Length;

        public void DeleteFile(string path)
        {
            if (File.Exists(path)) File.Delete(path);
        }

        public void DeleteDirectory(string path, bool recursive)
        {
            if (Directory.Exists(path)) Directory.Delete(path, recursive);
        }

        public string[] GetFiles(string path, string searchPattern, SearchOption searchOption)
            => Directory.GetFiles(path, searchPattern, searchOption);
    }
}
