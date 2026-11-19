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
/// Write a value value into a field of every MonoBehaviour of a script, e.g.
/// <c>new RewriteRule("PlayMaker", "PlayMakerFSM", "fsm.preprocessed", false)</c>.
/// </summary>
public sealed class RewriteRule {
    /// <summary>Without <c>.dll</c></summary>
    public string Assembly { get; }

    /// <summary><c>Namespace.Class</c></summary>
    public string Script { get; }

    /// <summary>Field names from the MonoBehaviour, separated by dots.</summary>
    public string Field { get; }

    private readonly string valueJson;

    public RewriteRule(string assembly, string script, string field, bool value)
        : this(assembly, script, field, value ? "true" : "false") { }

    public RewriteRule(string assembly, string script, string field, long value)
        : this(assembly, script, field, value.ToString(CultureInfo.InvariantCulture)) { }

    private RewriteRule(string assembly, string script, string field, string valueJson) {
        Assembly = assembly;
        Script = script;
        Field = field;
        this.valueJson = valueJson;
    }

    internal static string? ToJson(IReadOnlyList<RewriteRule> rules) {
        if (rules.Count == 0) return null;
        return "[" + string.Join(",", rules.Select(rule =>
            $"{{\"assembly\":{Str(rule.Assembly)},\"script\":{Str(rule.Script)},\"field\":{Str(rule.Field)},\"value\":{rule.valueJson}}}")) + "]";

        static string Str(string s) => "\"" + s.Replace(@"\", @"\\").Replace(@"""", @"\""") + "\"";
    }
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
        var rewriteRules = mode is RepackMode.AssetBundle assetBundle ? RewriteRule.ToJson(assetBundle.RewriteRules) : null;
        byte* error = null;
        CStats stats;
        int bundleSize, catalogSize;
        byte* bundleData;
        byte* catalogData;
        byte* dependencies;
        fixed (byte* namePtr = Utf8(name))
        fixed (byte* objectsPtr = Utf8(objectsJson))
        fixed (byte* rewriteRulesPtr = Utf8OrNull(rewriteRules)) {
            options.rewrite_rules = rewriteRulesPtr;
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
