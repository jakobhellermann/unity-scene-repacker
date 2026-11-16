using System;
using System.Runtime.InteropServices;
using System.Text;

namespace UnitySceneRepacker;

/// <summary>Compression level of the generated bundles.</summary>
public enum Compression : byte {
    None = 0,
    Lz4 = 2,
}

/// <summary>An error reported by unity-scene-repacker.</summary>
public sealed class RepackException(string message) : Exception(message);

/// <summary>The kind of bundle to repack into, with the options specific to it.</summary>
public abstract class RepackMode {
    private RepackMode() { }

    internal abstract RepackOptions ToOptions(Compression compression);

    /// <summary>A bundle of the filtered scenes, loaded using <c>SceneManager.LoadScene</c>.</summary>
    public sealed class SceneBundle : RepackMode {
        /// <summary>Start the root gameobjects disabled.</summary>
        public bool DisableRoots { get; set; }

        internal override RepackOptions ToOptions(Compression compression) => new() {
            mode = 0,
            compression = (byte)compression,
            disable_roots = DisableRoots,
        };
    }

    /// <summary>A bundle of the objects as individual assets, loaded using <c>bundle.LoadAsset</c>.</summary>
    public sealed class AssetBundle : RepackMode {
        /// <summary>Start the root gameobjects disabled.</summary>
        public bool DisableRoots { get; set; }

        /// <summary>Embed type trees, so the bundle loads in similar Unity versions.</summary>
        public bool WriteTypetrees { get; set; }

        /// <summary>
        /// Reference the game files the bundle depends on as <c>deps/{file}.bundle</c> and return an
        /// Addressables catalog for it, for use in another game. Implies <see cref="WriteTypetrees"/>.
        /// </summary>
        public bool EmitCatalog { get; set; }

        internal override RepackOptions ToOptions(Compression compression) => new() {
            mode = 1,
            compression = (byte)compression,
            disable_roots = DisableRoots,
            write_typetrees = WriteTypetrees,
            emit_catalog = EmitCatalog,
        };
    }

    /// <summary>An empty bundle referencing the objects in the original level files, loaded using <c>bundle.LoadAsset</c>.</summary>
    public sealed class ShallowAssetBundle : RepackMode {
        internal override RepackOptions ToOptions(Compression compression) => new() {
            mode = 2,
            compression = (byte)compression,
        };
    }

}

public sealed class RepackStats(int objectsBefore, int objectsAfter) {
    /// <summary>Object count in the read scene files.</summary>
    public int ObjectsBefore { get; } = objectsBefore;

    /// <summary>Object count kept in the bundle.</summary>
    public int ObjectsAfter { get; } = objectsAfter;
}

/// <summary>The result of <see cref="GameRepacker.Repack"/>.</summary>
public sealed class RepackResult(byte[] bundle, byte[]? catalog, string[] dependencyFiles, RepackStats stats) {
    /// <summary>The resulting asset bundle.</summary>
    public byte[] Bundle { get; } = bundle;

    /// <summary>The Addressables catalog, with <see cref="RepackMode.AssetBundle.EmitCatalog"/>.</summary>
    public byte[]? Catalog { get; } = catalog;

    /// <summary>The game files the catalog loads as <c>deps/{file}.bundle</c>, see <see cref="GameRepacker.BuildDependencyFileBundle"/>.</summary>
    public string[] DependencyFiles { get; } = dependencyFiles;

    public RepackStats Stats { get; } = stats;
}

/// <summary>Main interface for generating repacks.</summary>
public sealed unsafe class GameRepacker : IDisposable {
    private Game* game;

    /// <exception cref="RepackException">The game could not be opened.</exception>
    public GameRepacker(string gameDir) {
        byte* error = null;
        fixed (byte* dir = Utf8(gameDir)) {
            game = NativeMethods.game_open(dir, &error);
        }
        ThrowIfError(error);
    }

    /// <summary>Repacks the given objects into a bundle.</summary>
    /// <param name="name">The bundle name, which should be unique among all loaded bundles.</param>
    /// <param name="objectsJson">JSON5 object mapping scene names to the gameobject paths to include.</param>
    /// <param name="mode">The kind of bundle, and options.</param>
    /// <param name="compression">The compression level of the bundle.</param>
    /// <exception cref="RepackException"/>
    public RepackResult Repack(string name, string objectsJson, RepackMode mode, Compression compression = Compression.Lz4) {
        var handle = Handle();
        var options = mode.ToOptions(compression);
        byte* error = null;
        CStats stats;
        int bundleSize, catalogSize;
        byte* bundleData;
        byte* catalogData;
        byte* dependencies;
        fixed (byte* namePtr = Utf8(name))
        fixed (byte* objectsPtr = Utf8(objectsJson)) {
            NativeMethods.repack(
                handle,
                namePtr,
                objectsPtr,
                &options,
                &error,
                &stats,
                &bundleSize,
                &bundleData,
                &catalogSize,
                &catalogData,
                &dependencies
            );
        }
        ThrowIfError(error);

        var bundle = TakeArray(bundleSize, bundleData);
        var catalog = catalogData == null ? null : TakeArray(catalogSize, catalogData);
        var dependencyFiles = dependencies == null
            ? []
            : TakeString(dependencies).Split(['\n'], StringSplitOptions.RemoveEmptyEntries);
        return new RepackResult(bundle, catalog, dependencyFiles, new RepackStats(stats.objects_before, stats.objects_after));
    }

    /// <summary>Builds the bundle <c>deps/{file}.bundle</c> of a game file, as referenced by <see cref="RepackResult.Catalog"/>.</summary>
    /// <param name="file">The game file, e.g. <c>sharedassets0.assets</c>.</param>
    /// <param name="compression">The compression level of the bundle.</param>
    /// <exception cref="RepackException"/>
    public byte[] BuildDependencyFileBundle(string file, Compression compression = Compression.Lz4) {
        var handle = Handle();
        byte* error = null;
        int size;
        byte* data;
        fixed (byte* filePtr = Utf8(file)) {
            NativeMethods.dependency_file_bundle(handle, filePtr, (byte)compression, &error, &size, &data);
        }
        ThrowIfError(error);
        return TakeArray(size, data);
    }

    private Game* Handle() {
        if (game == null) throw new ObjectDisposedException(nameof(GameRepacker));
        return game;
    }

    /// <summary>Version of the generated bundles and catalogs, changes whenever repacking the same input produces different output.</summary>
    public static uint FormatVersion => NativeMethods.format_version();

    public void Dispose() {
        Free();
        GC.SuppressFinalize(this);
    }

    ~GameRepacker() => Free();

    private void Free() {
        if (game == null) return;
        NativeMethods.game_free(game);
        game = null;
    }

    private static byte[] Utf8(string value) => Encoding.UTF8.GetBytes(value + "\0");

    private static byte[] TakeArray(int size, byte* data) {
        var array = new byte[size];
        Marshal.Copy((IntPtr)data, array, 0, size);
        NativeMethods.free_array(size, data);
        return array;
    }

    private static string TakeString(byte* str) {
        var length = 0;
        while (str[length] != 0) length++;
        var value = Encoding.UTF8.GetString(str, length);
        NativeMethods.free_str(str);
        return value;
    }

    private static void ThrowIfError(byte* error) {
        if (error == null) return;
        throw new RepackException(TakeString(error));
    }
}
