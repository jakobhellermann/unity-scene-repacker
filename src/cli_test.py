from pathlib import Path
import UnityPy

project = Path("/home/jakob/.local/share/Steam/steamapps/common/Nine Sols/NineSols_Data")
a = UnityPy.load(str(project.joinpath("level4"))).file

env = UnityPy.load("/home/jakob/dev/games/unity/TestAssetBundles/Assets/AssetBundles/prefabbundle")

bundle = env.file


ressname = "CAB-4ea0d51637fb71e2a8c1b0e2845de941.resS"
normalname = "CAB-4ea0d51637fb71e2a8c1b0e2845de941"

ress = bundle.files[ressname]
del bundle.files[ressname]


cab = bundle.files[normalname]
for key, a in cab.objects.items():
    if a.class_id == 142:
        tt = a.read_typetree()
        tt["m_IsStreamedSceneAssetBundle"] = True
        # tt["m_PreloadTable"] = []
        a.save_typetree(tt)


out_path = Path("out/outbundle_test")
out_path.parent.mkdir(parents=True, exist_ok=True)
with open(out_path, "wb") as f:
    f.write(bundle.save())


x = UnityPy.load(str(out_path))
print(x.file.files["CAB-4ea0d51637fb71e2a8c1b0e2845de941"].objects[1].read().read_typetree())

# test = Environment("/home/jakob/dev/games/unity/TestAssetBundles/Assets/AssetBundles/scenebundle")
# for file in test.file.files:
#     sa = test.file.files[file]
#     if not file.endswith("sharedAssets"):
#         continue
#
#     if 2 in sa.objects:
#         print(sa.objects[2])

{
    "m_Name": "prefabbundle",
    "m_PreloadTable": [
        {"m_FileID": 1, "m_PathID": 10753},
        {"m_FileID": 0, "m_PathID": -9159025132242208922},
        {"m_FileID": 0, "m_PathID": -9057271397218105870},
        {"m_FileID": 0, "m_PathID": -8505394581247272048},
        {"m_FileID": 0, "m_PathID": -6562632805628259368},
        {"m_FileID": 0, "m_PathID": -2763732700081595008},
        {"m_FileID": 0, "m_PathID": 730281745353086902},
        {"m_FileID": 0, "m_PathID": 2133465013409289562},
        {"m_FileID": 0, "m_PathID": 3527102368900574781},
        {"m_FileID": 0, "m_PathID": 3598057180795072361},
        {"m_FileID": 0, "m_PathID": 9116012849008334905},
    ],
    "m_Container": [
        (
            "assets/circle.prefab",
            {"preloadIndex": 0, "preloadSize": 11, "asset": {"m_FileID": 0, "m_PathID": -2763732700081595008}},
        )
    ],
    "m_MainAsset": {"preloadIndex": 0, "preloadSize": 0, "asset": {"m_FileID": 0, "m_PathID": 0}},
    "m_RuntimeCompatibility": 1,
    "m_AssetBundleName": "prefabbundle",
    "m_Dependencies": [],
    "m_IsStreamedSceneAssetBundle": True,
    "m_ExplicitDataLayout": 0,
    "m_PathFlags": 7,
    "m_SceneHashes": [],
}
