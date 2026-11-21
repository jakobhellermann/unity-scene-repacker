using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace UnitySceneRepacker;

/// <summary>
/// A UnityFS v8 bundle read as v7, for a Unity that reads UnityFS only up to v7.
///
/// Rewrites the header, and applies the specified byte patches.
/// </summary>
public sealed class UnityFsV8AsV7Stream : Stream {
    private readonly Stream file;
    private readonly byte[] header;
    private readonly long blocksInfoFileStart;
    private readonly long blocksInfoLength;
    private readonly long dataFileStart;
    private readonly long length;
    private readonly BytePatch[] patches;
    private long position;

    internal UnityFsV8AsV7Stream(Stream file, UnityFsHeader source, string declaredRevision, BytePatch[] patches) {
        if (source.Version != 8) throw new NotSupportedException($"UnityFS v{source.Version} bundles, only v8");
        if ((source.Flags & (UnityFsHeader.BlocksInfoAtEnd | UnityFsHeader.OldWebPluginCompatibility)) != 0)
            throw new NotSupportedException($"UnityFS v{source.Version} flags {source.Flags:x}");

        this.file = file;
        this.patches = patches;
        blocksInfoFileStart = source.BlocksInfoStart;
        blocksInfoLength = source.BlocksInfoCompressedSize;
        dataFileStart = source.DataStart;

        // v7 doesn't know the blocks info padding flag; the data follows the blocks info directly
        var v7 = new UnityFsHeader {
            Version = 7,
            GenerationVersion = source.GenerationVersion,
            EngineVersion = declaredRevision,
            BlocksInfoCompressedSize = source.BlocksInfoCompressedSize,
            BlocksInfoSize = source.BlocksInfoSize,
            Flags = source.Flags & ~UnityFsHeader.BlocksInfoNeedPaddingAtStart,
        };
        length = v7.BlocksInfoStart + blocksInfoLength + (file.Length - dataFileStart);
        v7.TotalSize = length;
        header = v7.ToBytes();
    }

    /// <summary>The UnityFS format version of the bundle at <paramref name="offset"/> in <paramref name="bundlePath"/>.</summary>
    public static uint ReadVersion(string bundlePath, long offset = 0) {
        using var file = new FileStream(bundlePath, FileMode.Open, FileAccess.Read, FileShare.Read) { Position = offset };
        return UnityFsHeader.Read(file).Version;
    }

    /// <summary>How many <see cref="BytePatch"/>es the data gets.</summary>
    public int PatchCount => patches.Length;

    public override bool CanRead => true;
    public override bool CanSeek => true;
    public override bool CanWrite => false;
    public override long Length => length;

    public override long Position {
        get => position;
        set => position = value;
    }

    public override int Read(byte[] buffer, int offset, int count) {
        var total = 0;
        while (count > 0 && position < length) {
            int read;
            if (position < header.Length) {
                read = (int)Math.Min(count, header.Length - position);
                Buffer.BlockCopy(header, (int)position, buffer, offset, read);
            } else {
                var afterHeader = position - header.Length;
                var filePosition = afterHeader < blocksInfoLength
                    ? blocksInfoFileStart + afterHeader
                    : dataFileStart + (afterHeader - blocksInfoLength);
                var segmentEnd = afterHeader < blocksInfoLength ? header.Length + blocksInfoLength : length;
                file.Position = filePosition;
                read = file.Read(buffer, offset, (int)Math.Min(count, segmentEnd - position));
                if (read == 0) throw new EndOfStreamException();
                if (filePosition >= dataFileStart) ApplyPatches(filePosition - dataFileStart, buffer, offset, read);
            }

            position += read;
            offset += read;
            count -= read;
            total += read;
        }

        return total;
    }

    private void ApplyPatches(long dataPosition, byte[] buffer, int offset, int count) {
        var end = dataPosition + count;
        foreach (var patch in patches) {
            if (patch.Offset >= end) break;
            var patchEnd = patch.Offset + patch.Bytes.Length;
            if (patchEnd <= dataPosition) continue;
            var from = Math.Max(patch.Offset, dataPosition);
            var to = Math.Min(patchEnd, end);
            Buffer.BlockCopy(patch.Bytes, (int)(from - patch.Offset), buffer, offset + (int)(from - dataPosition), (int)(to - from));
        }
    }

    public override long Seek(long offset, SeekOrigin origin) {
        position = origin switch {
            SeekOrigin.Begin => offset,
            SeekOrigin.Current => position + offset,
            SeekOrigin.End => length + offset,
            _ => throw new ArgumentOutOfRangeException(nameof(origin)),
        };
        return position;
    }

    public override void Flush() { }
    public override void SetLength(long value) => throw new NotSupportedException();
    public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();

    protected override void Dispose(bool disposing) {
        if (disposing) file.Dispose();
        base.Dispose(disposing);
    }
}

/// The header of a UnityFS bundle
// Adapted from AssetsTools.NET: https://github.com/nesrak1/AssetsTools.NET/blob/main/AssetTools.NET/Standard/AssetsBundleFileFormat/AssetBundleHeader.cs
internal sealed class UnityFsHeader {
    public const uint BlocksInfoAtEnd = 0x80;
    public const uint OldWebPluginCompatibility = 0x100;
    public const uint BlocksInfoNeedPaddingAtStart = 0x200;

    private const string Signature = "UnityFS";

    public uint Version;
    public string GenerationVersion = "";
    public string EngineVersion = "";
    public long TotalSize;
    public uint BlocksInfoCompressedSize;
    public uint BlocksInfoSize;
    public uint Flags;

    private long Length => Signature.Length + 1 + 4 + GenerationVersion.Length + 1 + EngineVersion.Length + 1 + 8 + 4 + 4 + 4;

    // Padded to 16 bytes since version 7.
    public long BlocksInfoStart => Version >= 7 ? Align16(Length) : Length;

    public long DataStart {
        get {
            var end = BlocksInfoStart + BlocksInfoCompressedSize;
            return (Flags & BlocksInfoNeedPaddingAtStart) != 0 ? Align16(end) : end;
        }
    }

    public static UnityFsHeader Read(Stream stream) {
        var signature = ReadString(stream);
        if (signature != Signature) throw new NotSupportedException($"{signature} bundles");

        return new UnityFsHeader {
            Version = (uint)ReadBigEndian(stream, 4),
            GenerationVersion = ReadString(stream),
            EngineVersion = ReadString(stream),
            TotalSize = ReadBigEndian(stream, 8),
            BlocksInfoCompressedSize = (uint)ReadBigEndian(stream, 4),
            BlocksInfoSize = (uint)ReadBigEndian(stream, 4),
            Flags = (uint)ReadBigEndian(stream, 4),
        };
    }

    // Up to the start of the blocks info, padding included.
    public byte[] ToBytes() {
        var bytes = new MemoryStream();
        WriteString(bytes, Signature);
        WriteBigEndian(bytes, Version, 4);
        WriteString(bytes, GenerationVersion);
        WriteString(bytes, EngineVersion);
        WriteBigEndian(bytes, TotalSize, 8);
        WriteBigEndian(bytes, BlocksInfoCompressedSize, 4);
        WriteBigEndian(bytes, BlocksInfoSize, 4);
        WriteBigEndian(bytes, Flags, 4);
        bytes.SetLength(BlocksInfoStart);

        return bytes.ToArray();
    }

    private static long Align16(long offset) => (offset + 15) & ~15L;

    private static string ReadString(Stream stream) {
        var bytes = new List<byte>();
        for (var b = stream.ReadByte(); b != 0; b = stream.ReadByte()) {
            if (b < 0) throw new EndOfStreamException();
            bytes.Add((byte)b);
        }
        return Encoding.ASCII.GetString([..bytes]);
    }

    private static long ReadBigEndian(Stream stream, int size) {
        long value = 0;
        for (var i = 0; i < size; i++) {
            var b = stream.ReadByte();
            if (b < 0) throw new EndOfStreamException();
            value = value << 8 | (byte)b;
        }
        return value;
    }

    private static void WriteString(Stream stream, string value) {
        var bytes = Encoding.ASCII.GetBytes(value);
        stream.Write(bytes, 0, bytes.Length);
        stream.WriteByte(0);
    }

    private static void WriteBigEndian(Stream stream, long value, int size) {
        for (var i = size - 1; i >= 0; i--) stream.WriteByte((byte)(value >> (8 * i)));
    }
}
