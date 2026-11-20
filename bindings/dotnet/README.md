# unity-scene-repacker bindings

Repack objects from Unity scenes into asset bundles at runtime.

C# bindings for [unity-scene-repacker](https://github.com/jakobhellermann/unity-scene-repacker).

The package contains
- The native libraries `libunityscenerepacker.so`/`unityscrenerepacker.dll`/`libunityscenerepacker.dylib` per platform
- `UnitySceneRepacker.Bindings.dll` - C# bindings for the API as `GameRepacker`
- `UnitySceneRepacker.Addressables.dll` - Abstraction for loading a cached addressables repack as `RepackedAssets`

## Bindings

```csharp
using UnitySceneRepacker;

using var game = new GameRepacker(".../Hollow Knight/hollow_knight_Data");

// objectsJson: json with comments, scene name to gameobject paths, e.g. { "Tutorial_01": ["_Enemies/Crawler 2"] }
RepackResult Repack(string name, string objectsJson, RepackMode mode, Compression compression = Compression.Lz4);
// required for `EmitCatalog` mode
byte[] BuildDependencyFileBundle(string file, Compression compression = Compression.Lz4);

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

## Addressables

`UnitySceneRepacker.Addressables.dll` contains `RepackedAssets`, which repacks scenes on demand into a cache directory and loads the result through Addressables. 

```csharp
using var hk = new RepackedAssets("/path/to/Hollow Knight", cacheDir, "hk-in-silksong");
RepackedAsset crawler = hk.Request("Tutorial_01", "_Enemies/Crawler 2");
RepackedScene kingsPass = hk.RequestScene("Tutorial_01");
hk.Prepare();                       // optional: start repacking early, instead of on the first Load

var handle = crawler.Load();        // AsyncOperationHandle<GameObject>
yield return handle;
Object.Instantiate(handle.Result);
Addressables.Release(handle);

yield return kingsPass.Load(LoadSceneMode.Additive); // AsyncOperationHandle<SceneInstance>
```

Alternatively for more flexibility, the lower level `RepackedCatalog` can be used instead.

```cs
var catalog = RepackedCatalog.Load(catalogPath);
```

## Usage

To use this package, the native `.so`,`.dll`,`dylib` must be available at runtime next to the `UnitySceneRepacker.Bindings.dll`.
The native libraries and c# dlls are all in the `UnitySceneRepackerFiles` item, which you can copy like this:

```xml
<Copy SourceFiles="@(UnitySceneRepackerFiles)" DestinationFolder="$(PluginDir)" />
```
