using System;
using System.Collections;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Text;
using System.Threading.Tasks;
using UnityEngine;
using UnityEngine.AddressableAssets;
using UnityEngine.ResourceManagement.AsyncOperations;
using Debug = UnityEngine.Debug;

namespace UnitySceneRepacker;

/// Objects of another game, repacked on demand into a catalog in `cacheDir`. Everyone sharing a cache must use the
/// same <paramref name="rewriteRules"/>, as they share the dependency bundles.
public sealed class RepackedAssets(
    string gameDir,
    string cacheDir,
    string name,
    IReadOnlyList<RewriteRule>? rewriteRules = null
) : IDisposable {
    private readonly IReadOnlyList<RewriteRule> rewriteRules = rewriteRules ?? [];
    private readonly Dictionary<string, SortedSet<string>> requests = new();

    private readonly List<AsyncOperationHandle> loaded = [];
    private RepackedCatalog? catalog;
    private Exception? processFailure;

    /// A gameobject to include in the next <see cref="ProcessRequests"/>.
    public RepackedAsset Request(string scene, string path) {
        if (!requests.TryGetValue(scene, out var paths))
            requests[scene] = paths = new(StringComparer.Ordinal);

        paths.Add(path);
        return new(this, $"{name}/{scene}/{path}.prefab".ToLowerInvariant());
    }

    internal AsyncOperationHandle<GameObject> Load(string key) {
        if (catalog == null) {
            throw processFailure != null
                ? new InvalidOperationException($"processing {name} failed", processFailure)
                : new InvalidOperationException($"{name} has not been processed");
        }
        var handle = Addressables.LoadAssetAsync<GameObject>(key);
        loaded.RemoveAll(h => !h.IsValid());
        loaded.Add(handle);
        return handle;
    }

    /// Repacks all requests if they changed, builds the missing dependency bundles and (re)loads the catalog.
    public IEnumerator ProcessRequests() {
        loaded.RemoveAll(h => !h.IsValid());
        if (loaded.Count > 0)
            throw new InvalidOperationException($"{loaded.Count} assets of {name} are still loaded, release them before processing again");
        catalog?.Dispose();
        catalog = null;
        processFailure = null;

        var repack = Task.Run(RepackIntoCache);
        yield return new WaitUntil(() => repack.IsCompleted);
        if (repack.IsFaulted) {
            processFailure = repack.Exception!.InnerException!;
            throw processFailure;
        }

        var (catalogPath, summary) = repack.Result;
        Debug.Log($"[{nameof(RepackedAssets)}] {summary}");

        var load = RepackedCatalog.Load(catalogPath);
        yield return load;

        if (load.Status != AsyncOperationStatus.Succeeded) {
            processFailure = new($"loading {catalogPath} failed", load.OperationException);
            throw processFailure;
        }
        catalog = load.Result;
    }

    private (string catalogPath, string summary) RepackIntoCache() {
        var depsDir = Path.Combine(cacheDir, "deps");
        var stale = InvalidateIfStale(depsDir);

        var json = ToJson(requests);
        var requestsPath = Path.Combine(cacheDir, name + ".requests.json");
        var bundlePath = Path.Combine(cacheDir, name + ".bundle");
        var catalogPath = Path.Combine(cacheDir, name + ".catalog.bin");
        if (!stale && File.Exists(requestsPath) && File.ReadAllText(requestsPath) == json && File.Exists(bundlePath) && File.Exists(catalogPath))
            return (catalogPath, $"{name} is up to date");

        var total = Stopwatch.StartNew();
        using var game = new GameRepacker(gameDir);
        var result = game.Repack(name,
            json,
            new RepackMode.AssetBundle {
                EmitCatalog = true,
                RewriteRules = rewriteRules,
            });
        var repackTime = total.Elapsed;

        var dependencies = Stopwatch.StartNew();
        var missing = result.DependencyFiles.Where(file => !File.Exists(DependencyPath(depsDir, file))).ToArray();
        // ReSharper disable once AccessToDisposedClosure
        Parallel.ForEach(missing, file => WriteAtomic(
            DependencyPath(depsDir, file),
            game.BuildDependencyFileBundle(file, rewriteRules: rewriteRules)
        ));
        var dependencyTime = dependencies.Elapsed;

        WriteAtomic(bundlePath, result.Bundle);
        WriteAtomic(catalogPath, result.Catalog!);
        // written last, so an interrupted repack isn't taken as up to date
        WriteAtomic(requestsPath, Encoding.UTF8.GetBytes(json));

        var summary = $"repacked {name} in {total.ElapsedMilliseconds}ms: "
            + $"{result.Stats.ObjectsBefore} -> {result.Stats.ObjectsAfter} objects in {repackTime.TotalMilliseconds:0}ms, "
            + $"built {missing.Length} of {result.DependencyFiles.Length} dependencies in {dependencyTime.TotalMilliseconds:0}ms";
        return (catalogPath, summary);
    }

    private static string DependencyPath(string depsDir, string file) => Path.Combine(depsDir, file + ".bundle");

    private bool InvalidateIfStale(string depsDir) {
        var fingerprintPath = Path.Combine(cacheDir, "fingerprint");
        var fingerprint = $"{GameRepacker.FormatVersion}\n{Path.GetFullPath(gameDir)}\n{RewriteRule.ToJson(rewriteRules)}";
        if (File.Exists(fingerprintPath) && File.ReadAllText(fingerprintPath) == fingerprint) return false;

        if (Directory.Exists(depsDir)) Directory.Delete(depsDir, true);
        Directory.CreateDirectory(depsDir);
        WriteAtomic(fingerprintPath, Encoding.UTF8.GetBytes(fingerprint));

        return true;
    }

    /// Files are replaced instead of overwritten, so a loaded bundle or an interrupted write never sees partial data.
    private static void WriteAtomic(string path, byte[] data) {
        var tmp = path + ".tmp";
        File.WriteAllBytes(tmp, data);
        if (File.Exists(path)) File.Delete(path);
        File.Move(tmp, path);
    }

    private static string ToJson(Dictionary<string, SortedSet<string>> objects) {
        return "{"
            + string.Join(",",
                objects.Select(scene =>
                    Str(scene.Key) + ": [" + string.Join(",", scene.Value.Select(Str)) + "]"))
            + "}";

        static string Str(string s) => "\"" + s.Replace(@"\", @"\\").Replace(@"""", @"\""") + "\"";
    }

    /// Releases the assets still loaded and unloads the catalog.
    public void Dispose() {
        foreach (var handle in loaded.Where(h => h.IsValid())) Addressables.Release(handle);
        loaded.Clear();
        catalog?.Dispose();
        catalog = null;
    }
}

/// A requested gameobject of <see cref="RepackedAssets"/>.
public sealed class RepackedAsset {
    private readonly RepackedAssets owner;
    private readonly string key;

    internal RepackedAsset(RepackedAssets owner, string key) {
        this.owner = owner;
        this.key = key;
    }

    /// Loads the prefab after <see cref="RepackedAssets.ProcessRequests"/>, to be released with <see cref="Addressables.Release(AsyncOperationHandle)"/>.
    public AsyncOperationHandle<GameObject> Load() => owner.Load(key);
}
