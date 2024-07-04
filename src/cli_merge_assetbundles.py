from UnityPy.environment import Environment
from UnityPy.files import BundleFile
from UnityPy.enums import ArchiveFlags
from UnityPy import classes
from pathlib import Path
import copy


class Fake(object):
    def __init__(self, _class, **kwargs):
        self.__class__ = _class
        self.__dict__.update(kwargs)


def load_bundle(path) -> BundleFile:
    if isinstance(path, Path):
        path = str(path)
    return Environment(path).file


a = load_bundle("/home/jakob/dev/games/unity/TestAssetBundles/Assets/AssetBundles/prefab_a")
b = load_bundle("/home/jakob/dev/games/unity/TestAssetBundles/Assets/AssetBundles/prefab_b")


def merge(a, b):
    acab = a.files["CAB-3fbce1cb6d9f915253e8f713c155c6b3"]
    bcab = b.files["CAB-e6a343b52d62193e5ce6de9be1c1fdb2"]
    bres = b.files["CAB-e6a343b52d62193e5ce6de9be1c1fdb2.resS"]

    abundle = acab.objects.pop(1)
    bbundle = bcab.objects.pop(1)

    for b_pathid in bcab.objects:
        for a_pathid in acab.objects:
            if b_pathid == a_pathid:
                raise Exception(f"duplicate pathid {a_pathid}")

    typelen_a = len(acab.types)
    acab.types = acab.types + bcab.types

    for path_id, obj in bcab.objects.items():
        obj.type_id += typelen_a
        acab.objects[path_id] = obj

    mergedobjects = copy.copy(acab.objects)

    a_container = abundle.read().read_typetree()["m_Container"]
    b_container = bbundle.read().read_typetree()["m_Container"]

    mergedcontainers = a_container + b_container
    for container in mergedcontainers:
        container[1]["preloadIndex"] = 0
        container[1]["preloadSize"] = 0

    # construct AssetBundle
    preload_table = []
    preload_table = []
    container = mergedcontainers
    bundle_tt = {
        "m_Name": "prefab_a",
        "m_PreloadTable": preload_table,
        "m_Container": container,
        "m_MainAsset": {"preloadIndex": 0, "preloadSize": 0, "asset": {"m_FileID": 0, "m_PathID": 0}},
        "m_RuntimeCompatibility": 1,
        "m_AssetBundleName": "prefab_a",
        "m_Dependencies": [],
        "m_IsStreamedSceneAssetBundle": False,
        "m_ExplicitDataLayout": 0,
        "m_PathFlags": 7,
        "m_SceneHashes": [],
    }
    abundle.save_typetree(bundle_tt)

    mergedobjects[1] = abundle
    acab.objects = mergedobjects

    files = {"CAB-e6a343b52d62193e5ce6de9be1c1fdb2": acab, "CAB-e6a343b52d62193e5ce6de9be1c1fdb2.resS": bres}

    for id, obj in mergedobjects.items():
        obj = obj.read()
        print(obj)
        # if isinstance(obj, classes.Sprite):
        # print(obj)

    return Fake(
        BundleFile,
        signature="UnityFS",
        version=8,
        version_player="5.x.x",
        version_engine="2022.3.18f1",
        dataflags=ArchiveFlags.BlocksAndDirectoryInfoCombined | ArchiveFlags.BlockInfoNeedPaddingAtStart | 3,
        _block_info_flags=64,
        _uses_block_alignment=True,
        files=files,
    )


merged = merge(a, b)

out_path = Path("out/outbundle_merged")
if __name__ == "__main__":
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "wb") as f:
        f.write(merged.save())

    print("-- BUNDLE --")
    sanity = load_bundle(out_path)
    for cab_name, cab in sanity.files.items():
        if cab_name.endswith(".resS"):
            continue

        for obj in cab.objects.items():
            print(obj)
