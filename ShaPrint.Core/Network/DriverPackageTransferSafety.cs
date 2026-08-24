using System;

namespace ShaPrint.Core.Network
{
    /// <summary>
    /// Validation and sequencing rules shared by the driver package client and server.
    /// Package identifiers are used as cache directory names, so this check must happen
    /// before logging, path construction, or any substring operation.
    /// </summary>
    public static class DriverPackageIdValidator
    {
        public const int Length = 64;

        public static bool IsValid(string? packageId)
        {
            if (packageId is null || packageId.Length != Length)
                return false;

            foreach (char c in packageId)
            {
                bool hex = (c >= '0' && c <= '9')
                    || (c >= 'a' && c <= 'f')
                    || (c >= 'A' && c <= 'F');
                if (!hex)
                    return false;
            }

            return true;
        }
    }

    /// <summary>
    /// Strictly accepts one contiguous chunk sequence. The transfer protocol is
    /// intentionally ordered; accepting a duplicate or gap would make the final
    /// SHA-256 represent a different byte stream than the manifest.
    /// </summary>
    public sealed class DriverChunkSequence
    {
        private readonly int _totalChunks;
        private int _nextIndex;

        public DriverChunkSequence(int totalChunks)
        {
            _totalChunks = totalChunks;
        }

        public int AcceptedChunks => _nextIndex;
        public bool IsComplete => _totalChunks > 0 && _nextIndex == _totalChunks;

        public bool TryAccept(int chunkIndex, int declaredTotalChunks, out string error)
        {
            if (_totalChunks <= 0 || declaredTotalChunks != _totalChunks)
            {
                error = "Chunk total does not match the transfer.";
                return false;
            }

            if (chunkIndex != _nextIndex)
            {
                error = chunkIndex < _nextIndex
                    ? "Duplicate driver package chunk."
                    : "Missing or out-of-order driver package chunk.";
                return false;
            }

            if (_nextIndex >= _totalChunks)
            {
                error = "Driver package transfer has already completed.";
                return false;
            }

            _nextIndex++;
            error = string.Empty;
            return true;
        }
    }
}
