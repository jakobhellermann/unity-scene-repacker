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
using UnityEngine.ResourceManagement;
using UnityEngine.ResourceManagement.AsyncOperations;
using Debug = UnityEngine.Debug;

namespace UnitySceneRepacker;

/// Objects of another game, repacked on demand into a catalog in `cacheDir`. Everyone sharing a cache must use the
/// same game and <paramref name="rewriteRules"/>, as they share the dependency bundles. <paramref name="rewriteRules"/>
/// is called once, on the first Load or <see cref="Prepare"/>.
public sealed class RepackedAssets(
    string gameDir,
    string cacheDir,
    string name,
    Func<IReadOnlyList<RewriteRule>>? rewriteRules = null
) : IDisposable {
    // repacks sharing a cache write the same dependency bundles
    private static readonly object cacheLock = new();

    private readonly Dictionary<string, SortedSet<string>> requests = new();

    private readonly List<AsyncOperationHandle> loaded = [];
    private AsyncOperationHandle<RepackedCatalog> processing;

    /// A gameobject to include. Everything has to be requested before the first Load or <see cref="Prepare"/>.
    public RepackedAsset Request(string scene, string path) {
        if (processing.IsValid())
            throw new InvalidOperationException($"can't request more from {name} after the first Load or Prepare");

        if (!requests.TryGetValue(scene, out var paths))
            requests[scene] = paths = new(StringComparer.Ordinal);

        paths.Add(path);
        return new(this, $"{name}/{scene}/{path}.prefab".ToLowerInvariant());
    }

    /// Starts repacking in the background, so the first Load doesn't have to wait for it.
    public void Prepare() {
        if (processing.IsValid()) return;

        var rules = rewriteRules?.Invoke() ?? [];
        var rm = Addressables.ResourceManager;
        var repack = rm.StartOperation(new RepackOperation(() => {
            lock (cacheLock) return RepackIntoCache(rules);
        }), default);
        processing = rm.CreateChainOperation(repack,
            op => op.Status == AsyncOperationStatus.Succeeded
                ? RepackedCatalog.Load(op.Result)
                : Failed<RepackedCatalog>($"repacking {name}", op.OperationException));
    }

    internal AsyncOperationHandle<GameObject> Load(string key) {
        Prepare();
        var handle = Addressables.ResourceManager.CreateChainOperation(processing,
            op => op.Status == AsyncOperationStatus.Succeeded
                ? Addressables.LoadAssetAsync<GameObject>(key)
                : Failed<GameObject>($"processing {name}", op.OperationException));
        loaded.RemoveAll(h => !h.IsValid());
        loaded.Add(handle);
        return handle;
    }

    private static AsyncOperationHandle<T> Failed<T>(string what, Exception cause) =>
        Addressables.ResourceManager.CreateCompletedOperationWithException<T>(default!, new Exception($"{what} failed", cause));

    private sealed class RepackOperation(Func<(string catalogPath, string summary)> repack)
        : AsyncOperationBase<string>, IUpdateReceiver {
        private Task<(string catalogPath, string summary)>? task;
        private bool completed;

        protected override void Execute() => task = Task.Run(repack);

        protected override bool InvokeWaitForCompletion() {
            ((IAsyncResult)task!).AsyncWaitHandle.WaitOne();
            Update(0);
            return true;
        }

        public void Update(float unscaledDeltaTime) {
            if (completed || !task!.IsCompleted) return;
            completed = true;

            if (task.IsFaulted) {
                Complete(null!, false, task.Exception!.InnerException!);
                return;
            }
            var (catalogPath, summary) = task.Result;
            Debug.Log($"[{nameof(RepackedAssets)}] {summary}");
            Complete(catalogPath, true, errorMsg: null);
        }
    }

    private (string catalogPath, string summary) RepackIntoCache(IReadOnlyList<RewriteRule> rewriteRules) {
        var depsDir = Path.Combine(cacheDir, "deps");
        InvalidateIfStale(depsDir, rewriteRules);

        var json = ToJson(requests);
        var requestsPath = Path.Combine(cacheDir, name + ".requests.json");
        var bundlePath = Path.Combine(cacheDir, name + ".bundle");
        var catalogPath = Path.Combine(cacheDir, name + ".catalog.bin");
        if (File.Exists(requestsPath) && File.ReadAllText(requestsPath) == json && File.Exists(bundlePath) && File.Exists(catalogPath))
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

    private void InvalidateIfStale(string depsDir, IReadOnlyList<RewriteRule> rewriteRules) {
        var fingerprintPath = Path.Combine(cacheDir, "fingerprint");
        var fingerprint = $"{GameRepacker.FormatVersion}\n{Path.GetFullPath(gameDir)}\n{GameRepacker.GameFingerprint(gameDir)}\n{RewriteRule.ToJson(rewriteRules)}";
        if (File.Exists(fingerprintPath) && File.ReadAllText(fingerprintPath) == fingerprint) return;

        if (Directory.Exists(cacheDir)) Directory.Delete(cacheDir, true);
        Directory.CreateDirectory(depsDir);
        WriteAtomic(fingerprintPath, Encoding.UTF8.GetBytes(fingerprint));
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

        if (!processing.IsValid()) return;
        if (processing.IsDone) ReleaseProcessing(processing);
        else processing.Completed += ReleaseProcessing;
        processing = default;

        static void ReleaseProcessing(AsyncOperationHandle<RepackedCatalog> op) {
            if (op.Status == AsyncOperationStatus.Succeeded) op.Result.Dispose();
            Addressables.Release(op);
        }
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

    /// Loads the prefab, to be released with <see cref="Addressables.Release(AsyncOperationHandle)"/>.
    public AsyncOperationHandle<GameObject> Load() => owner.Load(key);
}
