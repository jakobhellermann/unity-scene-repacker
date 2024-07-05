# unity-scene-repacker

When modding a unity game, you often want to `Instantiate` objects from a scene that isn't loaded.
One solution for this, is to load all the scenes you're interested in at startup, copy the gameobjects somewhere and unload the scene.
This works, but is slow and memory intensive.

This project lets you read scenes from the distributed game, take only what you need, and package those objects into an [AssetBundle](https://docs.unity3d.com/Manual/AssetBundlesIntro.html) that you can load at runtime.

## Usage

```json
scenes.json
{
    "Logo": 0,
    "TitleScreenMenu": 1,
    "A0_S1_intro_morning": 2,
    "A0_S2_intro_night": 3,
    "A0_S3_altar": 4,
    "A0_S4_tutorial": 5,
    ...
}
```

```json
monsters.json
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
python src/cli.py \
    --scene-defs ninesols/scenes.json \
    --preloads ninesols/monsters.json \
    --output out/preloads.bundle \
    --game-dir '~/.local/share/Steam/steamapps/common/Nine Sols/NineSols_Data/'
```
