using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using UnityEngine.AddressableAssets;
using UnityEngine.AddressableAssets.Initialization;
using UnityEngine.AddressableAssets.ResourceLocators;
using UnityEngine.ResourceManagement.AsyncOperations;
using UnityEngine.ResourceManagement.ResourceProviders;
using Object = UnityEngine.Object;

namespace UnitySceneRepacker;

public sealed class RepackedCatalog : IDisposable {
    private const string MonoScriptsKey = "monoscripts";

    public IResourceLocator Locator { get; }

    private readonly string bundleName;
    private readonly AsyncOperationHandle<IList<IAssetBundleResource>> scripts;
    // Unity only binds a MonoBehaviour to an already loaded MonoScript, and unloads unreferenced ones.
    private readonly List<Object> monoScripts;

    private const string CatalogSuffix = ".catalog.bin";

    /// Load the `{bundle name}.catalog.bin` written by `--emit-dependency-addressables`, next to the bundles.
    public static AsyncOperationHandle<RepackedCatalog> Load(string catalogPath) {
        catalogPath = Path.GetFullPath(catalogPath);
        var fileName = Path.GetFileName(catalogPath);
        if (!fileName.EndsWith(CatalogSuffix))
            throw new ArgumentException($"{catalogPath} is not a {CatalogSuffix} file", nameof(catalogPath));

        var bundleName = fileName.Substring(0, fileName.Length - CatalogSuffix.Length);
        AddressablesRuntimeProperties.SetPropertyValue($"UnitySceneRepacker.{bundleName}", Path.GetDirectoryName(catalogPath));

        var catalog = Addressables.LoadContentCatalogAsync(catalogPath);
        return Addressables.ResourceManager.CreateChainOperation(catalog,
            op => op.Status == AsyncOperationStatus.Succeeded
                ? FromLocator(op.Result, bundleName)
                : Failed($"loading catalog {catalogPath}", op.OperationException));
    }

    private static AsyncOperationHandle<RepackedCatalog> FromLocator(IResourceLocator locator, string bundleName) {
        if (!locator.Locate(MonoScriptsKey, typeof(IAssetBundleResource), out var locations))
            locations = [];

        var scripts = Addressables.LoadAssetsAsync<IAssetBundleResource>(locations, null);

        return Addressables.ResourceManager.CreateChainOperation(scripts,
            op => op.Status == AsyncOperationStatus.Succeeded
                ? Addressables.ResourceManager.StartOperation(new LoadMonoScriptsOperation(locator, bundleName, op), op)
                : Failed("loading MonoScripts", op.OperationException));
    }

    private sealed class LoadMonoScriptsOperation(
        IResourceLocator locator,
        string bundleName,
        AsyncOperationHandle<IList<IAssetBundleResource>> scripts
    ) : AsyncOperationBase<RepackedCatalog> {
        protected override void Execute() {
            var requests = scripts.Result.Select(bundle => bundle.GetAssetBundle().LoadAllAssetsAsync()).ToList();
            if (requests.Count == 0) {
                Finish();
                return;
            }

            var pending = requests.Count;
            foreach (var request in requests) {
                request.completed += _ => {
                    if (--pending == 0) Finish();
                };
            }
            return;

            void Finish() {
                var monoScripts = requests.SelectMany(request => request.allAssets).ToList();
                Complete(new RepackedCatalog(locator, bundleName, scripts, monoScripts), true, errorMsg: null);
            }
        }
    }

    private static AsyncOperationHandle<RepackedCatalog> Failed(string what, Exception cause) =>
        Addressables.ResourceManager.CreateCompletedOperationWithException<RepackedCatalog>(null!, new Exception($"{what} failed", cause));

    private RepackedCatalog(
        IResourceLocator locator,
        string bundleName,
        AsyncOperationHandle<IList<IAssetBundleResource>> scripts,
        List<Object> monoScripts
    ) {
        Locator = locator;
        this.bundleName = bundleName;
        this.scripts = scripts;
        this.monoScripts = monoScripts;
        Addressables.ResourceManager.Acquire(scripts);
    }

    /// The keys of the repacked GameObjects, loadable in `Addressables.LoadAssetAsync`.
    public IEnumerable<string> Assets =>
        Locator.Keys.OfType<string>().Where(key => key.StartsWith(bundleName + "/", StringComparison.Ordinal));

    public void Dispose() {
        monoScripts.Clear();
        Addressables.Release(scripts);
        Addressables.RemoveResourceLocator(Locator);
    }
}
