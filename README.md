# unity-scene-repacker

When modding a unity game, you often want to `Instantiate` objects from a scene that isn't loaded.
One solution for this is to load all the scenes you're interested in at startup, copy the gameobjects somewhere and unload the scene.
This works, but is slow and memory intensive.

This project lets you read scenes from the distributed game, take only what you need, and package those objects into an [AssetBundle](https://docs.unity3d.com/Manual/AssetBundlesIntro.html) that you can load at runtime.

## Installation

```sh
# compile from source
cargo install --git https://github.com/jakobhellermann/unity-scene-repacker --branch rewrite
```

## Usage

```json
objects.json
{
    "A6_S3_Tutorial_And_SecretBoss_Remake": [
        "A6_S3/Room/Prefab/Enemy/StealthGameMonster_RunningHog (3)",
        "A6_S3/Room/Prefab/Enemy_2/\u5f29\u83c1\u82f1"
    ],
    "A0_S7_CaveReturned": [
        "A0_S7/Room/StealthGameMonster_TutorialDummyNonAttack"
    ],
    "AG_SG1": [
        "AG_SG1/Room/Shield Crawler Spawner \u4e1f\u91d1\u96fb\u87f2"
    ],
    ...
}
```

```sh
unity-scene-repacker 
    --steam-game 'Hollow Knight'
    --objects objects.json \
    --output mybundle.unity3d
```


```
Usage: unity-scene-repacker [OPTIONS] --objects <OBJECTS> <--game-dir <GAME_DIR>|--steam-game <STEAM_GAME>>

Options:
      --game-dir <GAME_DIR>
          Directory where the levels files are, e.g. steam/Hollow_Knight/hollow_knight_Data1

      --steam-game <STEAM_GAME>
          App ID or search term for the steam game to detect

      --objects <OBJECTS>
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
          

      --disable
          When true, all gameobjects in the scene will start out disabled

      --compression <COMPRESSION>
          Compression level to apply
          
          [default: none]

          Possible values:
          - none
          - lzma
          - lz4
          - lz4hc: Best compression at the cost of speed

  -o, --output <OUTPUT>
          [default: out.unity3d]

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```