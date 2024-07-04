from pathlib import Path
import UnityPy
import random

from prune import prune


scene_bundle = UnityPy.load("in/empty_scene_bundle").file
scene_file = scene_bundle.files["BuildPlayer-EmptyScene"]
prune(scene_file, ["Test"])


new_objects = []
for prev_id, reader in scene_file.items():
    new_objects.append((prev_id, reader))
print(new_objects)


project = Path("/home/jakob/.local/share/Steam/steamapps/common/Nine Sols/NineSols_Data")

env = UnityPy.load("/home/jakob/dev/games/unity/TestAssetBundles/Assets/AssetBundles/prefabbundle")
bundle = env.file

cabname = "CAB-4ea0d51637fb71e2a8c1b0e2845de941"
del bundle.files[cabname + ".resS"]

cab = bundle.files[cabname]

newids = [
    -9159025132242208922,
    -9057271397218105870,
    -8505394581247272048,
    -6562632805628259368,
    -2763732700081595008,
    730281745353086902,
    2133465013409289562,
    3527102368900574781,
    3598057180795072361,
    9116012849008334905,
]
assetbundle = cab.objects[1]

tt = assetbundle.read_typetree()
tt = {
    "m_Name": "prefabbundle",
    "m_PreloadTable": [
        {"m_FileID": 1, "m_PathID": 10753},
        # {"m_FileID": 0, "m_PathID": -9159025132242208922},
        # {"m_FileID": 0, "m_PathID": -9057271397218105870},
        # {"m_FileID": 0, "m_PathID": -8505394581247272048},
        #
        # {"m_FileID": 0, "m_PathID": -6562632805628259368},
        # {"m_FileID": 0, "m_PathID": -2763732700081595008},
        # {"m_FileID": 0, "m_PathID": 730281745353086902},
        # {"m_FileID": 0, "m_PathID": 2133465013409289562},
        # {"m_FileID": 0, "m_PathID": 3527102368900574781},
        # {"m_FileID": 0, "m_PathID": 3598057180795072361},
        # {"m_FileID": 0, "m_PathID": 9116012849008334905},
    ],
    "m_Container": [
        (
            "assets/circle.prefab",
            {"preloadIndex": 0, "preloadSize": 4, "asset": {"m_FileID": 0, "m_PathID": -9159025132242208922}},
        )
    ],
    "m_MainAsset": {"preloadIndex": 0, "preloadSize": 0, "asset": {"m_FileID": 0, "m_PathID": 0}},
    "m_RuntimeCompatibility": 1,
    "m_AssetBundleName": "prefabbundle",
    "m_Dependencies": [],
    "m_IsStreamedSceneAssetBundle": False,
    "m_ExplicitDataLayout": 0,
    "m_PathFlags": 7,
    "m_SceneHashes": [],
}
assetbundle.save_typetree(tt)
# cab.objects = {1: assetbundle}


print(new_objects)
cab.objects[-9159025132242208922] = new_objects[2][1]
# for oldid, obj in new_objects:
#     cab.objects[newids[oldid - 1]] = obj
#     oldtype = scene_file.types[obj.type_id]
#
#     newtypeid = len(cab.types)
#     obj.type_id = newtypeid
#     cab.types.append(oldtype)


out_path = Path("out/outbundle_test")
out_path.parent.mkdir(parents=True, exist_ok=True)
with open(out_path, "wb") as f:
    f.write(bundle.save())


x = UnityPy.load(str(out_path))
print(x.file.files)
print(x.file.files["CAB-4ea0d51637fb71e2a8c1b0e2845de941"].objects)
# print(x.file.files["CAB-4ea0d51637fb71e2a8c1b0e2845de941"].objects[1].read().read_typetree())

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
