from UnityPy.environment import Environment
from UnityPy.files import BundleFile
from UnityPy.classes import MonoBehaviour, MonoScript
from UnityPy.enums import ArchiveFlags, ClassIDType
from UnityPy import classes
from pathlib import Path
import random
import copy


def load_bundle(path) -> BundleFile:
    if isinstance(path, Path):
        path = str(path)
    return Environment(path).file


prefabb = load_bundle("/home/jakob/dev/games/unity/TestAssetBundles/Assets/AssetBundles/prefab_b")

bb = prefabb.files["CAB-e6a343b52d62193e5ce6de9be1c1fdb2"]
for obj in bb.objects.values():
    print(obj)

print()

fscene = load_bundle("/home/jakob/dev/games/unity/TestAssetBundles/Assets/AssetBundles/repro_scene")
fprefab = load_bundle("/home/jakob/dev/games/unity/TestAssetBundles/Assets/AssetBundles/repro_prefab")


print("-----SCENE-------")
print(fscene.files, "\n")
s = fscene.files["BuildPlayer-Repro"]
sa = fscene.files["BuildPlayer-Repro.sharedAssets"]

scene_mb: MonoBehaviour = s.objects[6].read()
scene_ms: MonoScript = sa.objects[3].read()

print(scene_mb, scene_mb.m_Script.__dict__)
print(scene_ms)

# print("--scene")
# for obj in s.objects.values():
#     print(obj.read())
# print("--scene.sharedAssets")
# for obj in sa.objects.values():
#     print(obj.read())

print("\n\n-----PREFAB------")
print(fprefab.files, "\n")
sprefab = fprefab.files["CAB-545f1f7e9e33b5389bc516b581e06d79"]
# for obj in sprefab.objects.values():
#    print(obj, obj.path_id)

prefab_mb: MonoBehaviour = sprefab.objects[1838317242174238564].read()
prefab_ms: MonoScript = sprefab.objects[3100192477084745482].read()

print(prefab_mb, prefab_mb.m_Script.__dict__)
print(prefab_ms)
