using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
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

/// <summary>
/// A change to every MonoBehaviour of the script <c>Namespace.Class</c> in <c>assembly</c> (without <c>.dll</c>).
/// </summary>
public sealed class RewriteRule {
    private readonly string json;

    private RewriteRule(string assembly, string script, string actionJson) {
        json = $"{{\"assembly\":{Str(assembly)},\"script\":{Str(script)},{actionJson}}}";
    }

    /// <summary>
    /// Write <paramref name="value"/> into <paramref name="field"/>, as field names separated by dots, e.g.
    /// <c>RewriteRule.SetField("PlayMaker", "PlayMakerFSM", "fsm.preprocessed", false)</c>.
    /// </summary>
    public static RewriteRule SetField(string assembly, string script, string field, bool value) =>
        new(assembly, script, $"\"field\":{Str(field)},\"value\":{(value ? "true" : "false")}");

    /// <inheritdoc cref="SetField(string, string, string, bool)"/>
    public static RewriteRule SetField(string assembly, string script, string field, long value) =>
        new(assembly, script, $"\"field\":{Str(field)},\"value\":{value.ToString(CultureInfo.InvariantCulture)}");

    /// <summary>
    /// Bind the MonoBehaviours to <paramref name="toScript"/> in <paramref name="toAssembly"/> instead.
    /// </summary>
    public static RewriteRule RemapScript(string assembly, string script, string toAssembly, string toScript) =>
        new(assembly, script, $"\"remap\":{{\"assembly\":{Str(toAssembly)},\"script\":{Str(toScript)}}}");

    internal static string? ToJson(IReadOnlyList<RewriteRule> rules) {
        if (rules.Count == 0) return null;
        return "[" + string.Join(",", rules.Select(rule => rule.json)) + "]";
    }

    private static string Str(string s) => Json.Str(s);
}

internal static class Json {
    internal static string Str(string s) => "\"" + s.Replace(@"\", @"\\").Replace(@"""", @"\""") + "\"";
}

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

        /// <summary>Also apply these to the dependency bundles, see <see cref="GameRepacker.BuildDependencyFileBundle"/>.</summary>
        public IReadOnlyList<RewriteRule> RewriteRules { get; set; } = [];

        /// <summary>
        /// Scenes repacked whole into the scene bundle <c>{name}.scenes</c>, keyed <c>{name}/{scene}</c> in the
        /// catalog. Requires <see cref="EmitCatalog"/>.
        /// </summary>
        public IReadOnlyList<string> Scenes { get; set; } = [];

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
public sealed class RepackResult(byte[] bundle, byte[]? sceneBundle, byte[]? catalog, string[] dependencyFiles, RepackStats stats) {
    /// <summary>The resulting asset bundle.</summary>
    public byte[] Bundle { get; } = bundle;

    /// <summary>The bundle of <see cref="RepackMode.AssetBundle.Scenes"/>, if there are any.</summary>
    public byte[]? SceneBundle { get; } = sceneBundle;

    /// <summary>The Addressables catalog, with <see cref="RepackMode.AssetBundle.EmitCatalog"/>.</summary>
    public byte[]? Catalog { get; } = catalog;

    /// <summary>The game files the catalog loads as <c>deps/{file}.bundle</c>, see <see cref="GameRepacker.BuildDependencyFileBundle"/>.</summary>
    public string[] DependencyFiles { get; } = dependencyFiles;

    public RepackStats Stats { get; } = stats;
}

/// <summary>The result of <see cref="GameRepacker.Info"/>.</summary>
public sealed class GameInfo(string bundleVersion, string unityVersion) {
    /// <summary>The game's version from its player settings, <c>Application.version</c> at runtime.</summary>
    public string BundleVersion { get; } = bundleVersion;

    public string UnityVersion { get; } = unityVersion;
}

/// <summary>A change to a bundle's bytes that keeps their size.</summary>
public readonly struct BytePatch(long offset, byte[] bytes) {
    /// <summary>From the start of the bundle's data, past its header and blocks info.</summary>
    public long Offset { get; } = offset;

    public byte[] Bytes { get; } = bytes;
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
        var assetBundle = mode as RepackMode.AssetBundle;
        var rewriteRules = assetBundle == null ? null : RewriteRule.ToJson(assetBundle.RewriteRules);
        var scenes = assetBundle == null || assetBundle.Scenes.Count == 0
            ? null
            : "[" + string.Join(",", assetBundle.Scenes.Select(Json.Str)) + "]";
        byte* error = null;
        CStats stats;
        int bundleSize, sceneBundleSize, catalogSize;
        byte* bundleData;
        byte* sceneBundleData;
        byte* catalogData;
        byte* dependencies;
        fixed (byte* namePtr = Utf8(name))
        fixed (byte* objectsPtr = Utf8(objectsJson))
        fixed (byte* rewriteRulesPtr = Utf8OrNull(rewriteRules))
        fixed (byte* scenesPtr = Utf8OrNull(scenes)) {
            options.rewrite_rules = rewriteRulesPtr;
            options.scenes = scenesPtr;
            NativeMethods.repack(
                handle,
                namePtr,
                objectsPtr,
                &options,
                &error,
                &stats,
                &bundleSize,
                &bundleData,
                &sceneBundleSize,
                &sceneBundleData,
                &catalogSize,
                &catalogData,
                &dependencies
            );
        }
        ThrowIfError(error);

        var bundle = TakeArray(bundleSize, bundleData);
        var sceneBundle = sceneBundleData == null ? null : TakeArray(sceneBundleSize, sceneBundleData);
        var catalog = catalogData == null ? null : TakeArray(catalogSize, catalogData);
        var dependencyFiles = dependencies == null
            ? []
            : TakeString(dependencies).Split(['\n'], StringSplitOptions.RemoveEmptyEntries);
        return new RepackResult(bundle, sceneBundle, catalog, dependencyFiles, new RepackStats(stats.objects_before, stats.objects_after));
    }

    /// <summary>Builds the bundle <c>deps/{file}.bundle</c> of a game file, as referenced by <see cref="RepackResult.Catalog"/>.</summary>
    /// <param name="file">The game file, e.g. <c>sharedassets0.assets</c>.</param>
    /// <param name="compression">The compression level of the bundle.</param>
    /// <param name="rewriteRules">The <see cref="RepackMode.AssetBundle.RewriteRules"/> of the bundles referencing it.</param>
    /// <exception cref="RepackException"/>
    public byte[] BuildDependencyFileBundle(
        string file,
        Compression compression = Compression.Lz4,
        IReadOnlyList<RewriteRule>? rewriteRules = null
    ) {
        var handle = Handle();
        byte* error = null;
        int size;
        byte* data;
        fixed (byte* filePtr = Utf8(file))
        fixed (byte* rewriteRulesPtr = Utf8OrNull(RewriteRule.ToJson(rewriteRules ?? []))) {
            NativeMethods.dependency_file_bundle(handle, filePtr, (byte)compression, rewriteRulesPtr, &error, &size, &data);
        }
        ThrowIfError(error);
        return TakeArray(size, data);
    }

    /// <exception cref="RepackException"/>
    public GameInfo Info() {
        var handle = Handle();
        byte* error = null;
        byte* bundleVersion = null;
        byte* unityVersion = null;
        NativeMethods.game_info(handle, &bundleVersion, &unityVersion, &error);
        ThrowIfError(error);

        return new GameInfo(TakeString(bundleVersion), TakeString(unityVersion));
    }

    /// <summary>Patches that make the objects of one of the game's bundles readable by an older Unity, in the order of their offsets.</summary>
    /// <param name="bundlePath">The bundle, which has to be stored uncompressed.</param>
    /// <param name="targetVersion">The Unity version reading the bundle, e.g. <c>2020.2.2f1</c>.</param>
    /// <exception cref="RepackException"/>
    public BytePatch[] CompatPatches(string bundlePath, string targetVersion) {
        var handle = Handle();
        byte* error = null;
        int count;
        CPatch* patches;
        fixed (byte* pathPtr = Utf8(bundlePath))
        fixed (byte* versionPtr = Utf8(targetVersion)) {
            NativeMethods.compat_patches(handle, pathPtr, versionPtr, &error, &count, &patches);
        }
        ThrowIfError(error);

        var result = new BytePatch[count];
        for (var i = 0; i < count; i++) {
            var bytes = new byte[patches[i].size];
            Marshal.Copy((IntPtr)patches[i].data, bytes, 0, bytes.Length);
            result[i] = new BytePatch(patches[i].offset, bytes);
        }
        NativeMethods.free_patches(count, patches);
        return result;
    }

    private Game* Handle() {
        if (game == null) throw new ObjectDisposedException(nameof(GameRepacker));
        return game;
    }

    /// <summary>Version of the generated bundles and catalogs, changes whenever repacking the same input produces different output.</summary>
    public static uint FormatVersion => NativeMethods.format_version();

    /// <exception cref="RepackException">The game could not be found.</exception>
    internal static string GameFingerprint(string gameDir) {
        byte* error = null;
        byte* fingerprint;
        fixed (byte* dir = Utf8(gameDir)) {
            fingerprint = NativeMethods.game_fingerprint(dir, &error);
        }
        ThrowIfError(error);
        return TakeString(fingerprint);
    }

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

    private static byte[]? Utf8OrNull(string? value) => value == null ? null : Utf8(value);

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
