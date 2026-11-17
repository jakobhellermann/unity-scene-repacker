# unity-scene-repacker bindings

Repack objects from Unity scenes into asset bundles at runtime.

C# bindings for [unity-scene-repacker](https://github.com/jakobhellermann/unity-scene-repacker).

```csharp
using UnitySceneRepacker;

using var game = new RepackGame(".../Hollow Knight/hollow_knight_Data");

RepackResult Repack(string name, string objectsJson, RepackMode mode, Compression compression = Compression.Lz4);
byte[] BuildDependencyFileBundle(string file, Compression compression = Compression.Lz4);

// objectsJson: json with comments, scene name to gameobject paths, e.g. { "Tutorial_01": ["_Enemies/Crawler 2"] }

new RepackMode.SceneBundle { DisableRoots };
new RepackMode.AssetBundle { DisableRoots, WriteTypetrees, EmitCatalog };
new RepackMode.ShallowAssetBundle();

class RepackResult {
    byte[] Bundle;            // <name>.bundle
    byte[]? Catalog;          // catalog.bin addressables, when EmitCatalog is set
    string[] DependencyFiles; // files necessary for BuildDependencyFileBundle, expected at deps/<file>.bundle
    RepackStats Stats;
}
```

The catalog from `EmitCatalog` loads bundles from the directory specified as the Addressables runtime property
`UnitySceneRepacker.<name>`.

## Addressables

`UnitySceneRepacker.Addressables.dll` repacks into a cache directory and loads the result through Addressables. Games
without Addressables only need `UnitySceneRepacker.Bindings.dll`.

```csharp
using var hk = new RepackedAssets("/path/to/Hollow Knight", cacheDir, "hk-in-silksong");
RepackedAsset crawler = hk.Request("Tutorial_01", "_Enemies/Crawler 2");
yield return hk.Process();          // repacks if the requests changed, builds missing dependencies, loads the catalog

var handle = crawler.Load();        // AsyncOperationHandle<GameObject>
yield return handle;
Object.Instantiate(handle.Result);
Addressables.Release(handle);

RepackedCatalog.LoadAsync(catalogPath); // a catalog written by the CLI with --emit-dependency-addressables
```

The native libraries are in `runtimes/<rid>/native`. To use only those, reference the package with
`ExcludeAssets="compile;runtime"`.

Mono loads native libraries next to the assembly instead. Plugins that ship one folder for all platforms, like BepInEx
plugins, copy the assemblies and native libraries of `@(UnitySceneRepackerFiles)` there:

```xml
<Copy SourceFiles="@(UnitySceneRepackerFiles)" DestinationFolder="$(PluginDir)" />
```
