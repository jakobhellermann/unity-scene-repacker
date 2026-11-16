# unity-scene-repacker bindings

Repack objects from Unity scenes into asset bundles at runtime.

C# bindings for [unity-scene-repacker](https://github.com/jakobhellermann/unity-scene-repacker).

## Public API

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

When the catalog from `EmitCatalog` is used, you need to set `AddressablesRuntimeProperties.SetPropertyValue` for
`UnitySceneRepacker.<name>` to the path containing the bundle and `deps/<file>.bundle`.

To use this package, the native `.so`,`.dll`,`dylib` must be available at runtime next to the `UnitySceneRepacker.Bindings.dll`.
The native libraries and c# dlls are all in the `UnitySceneRepackerFiles` item, which you can copy like this:

```xml
<Copy SourceFiles="@(UnitySceneRepackerFiles)" DestinationFolder="$(PluginDir)" />
```
