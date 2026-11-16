# unity-scene-repacker

![demo asciicast](./docs/demo-cast.svg)

When modding a unity game, you often want to `Instantiate` objects from a scene that isn't loaded.
One solution for this is to load all the scenes you're interested in at startup, copy the gameobjects somewhere and unload the scene.
This works, but is slow and memory intensive.

This project lets you read scenes from the distributed game, take only what you need, and package those objects into an [AssetBundle](https://docs.unity3d.com/Manual/AssetBundlesIntro.html) that you can load at runtime.

Check out the changelog at [docs/CHANGELOG.md](./docs/CHANGELOG.md).

## Installation

```sh
uv tool install unity-scene-repacker
```

## Usage

`unity-scene-repacker` can repack objects into two kinds of [`AssetBundle`](https://docs.unity3d.com/ScriptReference/AssetBundle.html)s.
- Scene bundles contain the original scenes, filtered down to only the requested objects.
- Asset bundles allow you to selectively load objects without having to load all of them, but are a bit more experimental.

### Scene bundle

```jsonc
objects.json
{
  "Fungus1_12": [
    "simple_grass",
    "green_grass_2",
    "green_grass_3",
    "green_grass_1 (1)"
  ],
  "White_Palace_01": [
    "WhiteBench",
    "White_ Spikes"
  ]
}
```

```sh
unity-scene-repacker \
    --steam-game 'Hollow Knight' \
    --scene-objects objects.json \
    --mode scene \
    --output mybundle.unity3d
```


```cs
const string bundleName = "mybundle";
var bundle = AssetBundle.LoadFromFile(path);

// load single scene
SceneManager.LoadScene($"Assets/unity-scene-repacker/{bundleName}/Fungus1_12.unity")

// load add scenes
foreach (string scenePath in bundle.GetAllScenePaths()) {
    string sceneName = Path.GetFileNameWithoutExtension(scenePath);
    SceneManager.LoadScene(sceneName, LoadSceneMode.Additive);
    var scene = USceneManager.GetSceneByPath(scenePath);
    var roots = scene.GetRootGameObjects();
}
bundle.Unload();
```

### Asset bundle

Asset bundles allow you to selectively load objects, without having to load the others into memory.
However, the implementation is a bit more experimental, so if something doesn't work, try the scene output instead.

```sh
unity-scene-repacker \
    --steam-game 'Hollow Knight' \
    --scene-objects objects.json \
    --mode asset \
    --output mybundle.unity3d
```


```cs
var bundle = AssetBundle.LoadFromFile(path);
var prefab = bundle.LoadAsset<GameObject>("Fungus1_12/simple_grass");
var object = Instantiate(prefab);
bundle.Unload();
```

The `asset` output mode also allows you to additionally specify arbitrary other objects (e.g. `ScriptableObject`s) by name:

```json
{
  "FXDealerMaterialTag": ["[FXDealer] 0_YeeAttack _PostureDecrease"]
}
```

```sh
unity-scene-repacker ... --extra-objects objects-by-type.json --mode asset
```

```cs
var object = bundle.LoadAsset<FXDealerMaterialTag>("ExtraObjects/FXDealerMaterialTag/[FXDealer] 0_YeeAttack _PostureDecrease.prefab");
```

### Emit Dependencies for an Asset Bundle

When loading an assetbundle, the dependencies (`sharedassetsXX.assets`, `globalgamemanagers`) of that bundle must be loaded before the asset bundle.

This is a problem when attempting to repack the files from one game and loading it into another.

When you specify `--emit-dependency-addressables`, unity-scene-repacker will pack the required dependencies into bundles in the out directory, and write an 
[Addressables](https://docs.unity3d.com/Packages/com.unity.addressables@4.2/manual/AddressableAssetsOverview.html) catalog for the repacked objects and their 
dependencies:

```sh
unity-scene-repacker ... --output out/hk-in-silksong.bundle --mode asset --emit-dependency-addressables 
```
```
out
├── deps
│   ├── globalgamemanagers.assets.bundle
│   ├── resources.assets.bundle
│   ├── sharedassets0.assets.bundle
│   ├── sharedassets1.assets.bundle
│   └── sharedassets6.assets.bundle
├── hk-in-silksong.bundle
└── hk-in-silksong.catalog.bin
```

```
# catalog keys
monoscripts # must be loaded and instantiated before
hk-in-silksong/tutorial_01/_enemies/crawler 2.prefab # bundlename/path.prefab per repacked object
```

Example usage:

```cs
var load = RepackedCatalog.LoadAsync(catalogPath);
yield return load;
using var catalog = load.Result;

foreach (var asset in catalog.Assets) {
    Log.Info(asset);
}

var prefab = Addressables.LoadAssetAsync<GameObject>("hk-in-silksong/tutorial_01/_enemies/crawler 2.prefab");
yield return prefab;
var hero = HeroController.instance ? HeroController.instance.transform.position : Vector3.zero;
var go = Object.Instantiate(prefab.Result, hero + Vector3.right * 5, prefab.Result.transform.rotation);

yield return new WaitForSecondsRealtime(5f);
Object.Destroy(go);
Addressables.Release(prefab);
```

<details>
    <summary>RepackedCatalog.cs</summary>

```cs
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using JetBrains.Annotations;
using UnityEngine.AddressableAssets;
using UnityEngine.AddressableAssets.Initialization;
using UnityEngine.AddressableAssets.ResourceLocators;
using UnityEngine.ResourceManagement.AsyncOperations;
using UnityEngine.ResourceManagement.ResourceProviders;
using Object = UnityEngine.Object;

namespace UnitySceneRepacker;

[PublicAPI]
public sealed class RepackedCatalog : IDisposable {
    private const string MonoScriptsKey = "monoscripts";

    public IResourceLocator Locator { get; }
    
    private readonly string bundleName;
    private readonly AsyncOperationHandle<IList<IAssetBundleResource>> scripts;
    // Unity only binds a MonoBehaviour to an already loaded MonoScript, and unloads unreferenced ones.
    private readonly List<Object> monoScripts = [];

    private const string CatalogSuffix = ".catalog.bin";

    /// Load the `{bundle name}.catalog.bin` written by `--emit-dependency-addressables`, next to the bundles.
    public static AsyncOperationHandle<RepackedCatalog> LoadAsync(string catalogPath) {
        catalogPath = Path.GetFullPath(catalogPath);
        var fileName = Path.GetFileName(catalogPath);
        if (!fileName.EndsWith(CatalogSuffix))
            throw new ArgumentException($"{catalogPath} is not a {CatalogSuffix} file", nameof(catalogPath));
        
        var bundleName = fileName[..^CatalogSuffix.Length];
        AddressablesRuntimeProperties.SetPropertyValue($"UnitySceneRepacker.{bundleName}", Path.GetDirectoryName(catalogPath));
        
        var catalog = Addressables.LoadContentCatalogAsync(catalogPath);
        return Addressables.ResourceManager.CreateChainOperation(catalog, op => op.Status == AsyncOperationStatus.Succeeded
            ? FromLocatorAsync(op.Result, bundleName)
            : Failed($"loading catalog {catalogPath}", op.OperationException));
    }

    private static AsyncOperationHandle<RepackedCatalog> FromLocatorAsync(IResourceLocator locator, string bundleName) {
        if (!locator.Locate(MonoScriptsKey, typeof(IAssetBundleResource), out var locations))
            locations = [];
        
        var scripts = Addressables.LoadAssetsAsync<IAssetBundleResource>(locations, null);
        
        return Addressables.ResourceManager.CreateChainOperation(scripts, op => op.Status == AsyncOperationStatus.Succeeded
            ? Addressables.ResourceManager.CreateCompletedOperation(new RepackedCatalog(locator, bundleName, op), string.Empty)
            : Failed("loading MonoScripts", op.OperationException));
    }

    private static AsyncOperationHandle<RepackedCatalog> Failed(string what, Exception cause) =>
        Addressables.ResourceManager.CreateCompletedOperationWithException<RepackedCatalog>(null!, new Exception($"{what} failed", cause));

    private RepackedCatalog(IResourceLocator locator, string bundleName, AsyncOperationHandle<IList<IAssetBundleResource>> scripts) {
        Locator = locator;
        this.bundleName = bundleName;
        this.scripts = scripts;
        Addressables.ResourceManager.Acquire(scripts);
        foreach (var bundle in scripts.Result) monoScripts.AddRange(bundle.GetAssetBundle().LoadAllAssets());
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
```

</details>


### CLI Reference

```
Usage: unity-scene-repacker [OPTIONS] <--game-dir <GAME_DIR>|--steam-game <STEAM_GAME>>
       unity-scene-repacker <COMMAND>

Commands:
  check-scripts  Check whether the scripts used in a repacked bundle exist with the same fields in another game
  help           Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Game options:
      --game-dir <GAME_DIR>
          Directory where the levels files are, e.g. steam/Hollow_Knight/hollow_knight_Data

      --steam-game <STEAM_GAME>
          App ID or search term for the steam game to detect

Repack options:
      --scene-objects <SCENE_OBJECTS>
          Path to JSON file, containing a map of scene name to a list of gameobject paths to include
          
            {
              "Fungus1_12": [
                "simple_grass",
                "green_grass_2",
              ],
              "White_Palace_01": [
                "WhiteBench",
              ]
            }
          

      --extra-objects <EXTRA_OBJECTS>
          Path to JSON file, containing a map of C# type to monobehaviour names. Useful for scriptable objects etc., which do not exist in the transform hierarchy.
          
            {
              "FXDealerMaterialTag": ["[FXDealer] 0_YeeAttack _PostureDecrease"]
            }
          

Output options:
      --mode <MODE>
          Possible values:
          - scene:         A scene asset bundle, containing the original scenes filtered. Load using LoadScene
          - asset:         An asset bundle containing individual assets you can load using LoadAsset. The objects are copied from the original level files
          - asset-shallow: An asset bundle containing individual assets you can load using LoadAsset. The bundle is completely empty, and only references the original game level files
          
          [default: scene]

      --disable
          When true, all gameobjects in the scene will start out disabled

      --compression <COMPRESSION>
          Compression level to apply

          Possible values:
          - none
          - lzma
          - lz4:   Fast compression, larger output than lz4hc
          - lz4hc: Best compression at the cost of speed
          
          [default: lz4]

  -o, --output <OUTPUT>
          [default: out.unity3d]

      --bundle-name <BUNDLE_NAME>
          Name to give the assetbundle. Should be unique for your game

      --write-typetrees
          When set, write the resulting bundle with typetree information embedded.
          
          This lets unity load asset bundles for different unity versions, as long as they are similar enough. Larger version differences might not work due to format changes to e.g. shaders, materials and animations.

      --emit-dependency-addressables
          When enabled, the game files the bundle depends on get copied to deps/ next to the output, and an Addressables catalog <bundle name>.catalog.bin is written referencing the repacked objects and their dependencies.
          
          The catalog loads bundles from the Addressables runtime property {UnitySceneRepacker.<bundle name>}. Implies --write-typetrees.
```
