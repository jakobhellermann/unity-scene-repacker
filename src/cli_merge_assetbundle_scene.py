from UnityPy.environment import Environment
from UnityPy.files import BundleFile
from UnityPy.enums import ArchiveFlags, ClassIDType
from pathlib import Path
import copy

from prune import get_root_objs2


class Fake(object):
    def __init__(self, _class, **kwargs):
        self.__class__ = _class
        self.__dict__.update(kwargs)


def load_bundle(path) -> BundleFile:
    if isinstance(path, Path):
        path = str(path)
    return Environment(path).file


# def generate_path_id():
#     while True:
#         uid = random.randint(-(2**16), 2**16 - 1)
#         return uid
#         # if uid not in objects:
#         #     return uid
#
#
# random.seed(42)
# b_id_remap = [generate_path_id() for _ in range(0, 200000)]
#
#
# def remap_path_id(id: int) -> int:
#     return b_id_remap[id - 1]
#
#
# def remap_path_in_tt(path) -> int:
#     assert path["m_FileID"] == 0
#     path["m_PathID"] = remap_path_id(path["m_PathID"])

# b_objects_remapped = {remap_path_id(id): obj for id, obj in bcab.objects.items()}

# for id, obj in b_objects_remapped.items():
#    obj.path_id = id

#    if obj.type == ClassIDType.GameObject:
#        tt = obj.read_typetree()
#        for component in tt["m_Component"]:
#            remap_path_in_tt(component["component"])
#        obj.save_typetree(tt)
#    elif (
#        obj.type == ClassIDType.Transform
#        or obj.type == ClassIDType.RectTransform
#        or obj.type == ClassIDType.SpriteRenderer
#        or obj.type == ClassIDType.MonoBehaviour
#        or obj.type == ClassIDType.CanvasRenderer
#        or obj.type == ClassIDType.CircleCollider2D
#        or obj.type == ClassIDType.PolygonCollider2D
#        or obj.type == ClassIDType.BoxCollider2D
#        or obj.type == ClassIDType.MeshRenderer
#        or obj.type == ClassIDType.MeshFilter
#        or obj.type == ClassIDType.ParticleSystemRenderer
#        or obj.type == ClassIDType.Animator
#        or obj.type == ClassIDType.Rigidbody2D
#        or obj.type == ClassIDType.CanvasGroup
#        or obj.type == ClassIDType.TilemapRenderer
#    ):
#        try:
#            tt = obj.read_typetree()
#            remap_path_in_tt(tt["m_GameObject"])
#            obj.save_typetree(tt)
#        except Exception as e:
#            e = Exception(f"failed to read typetree in {obj.type.name}: {e}")
#            print(e)
#    # else:
#    #     try:
#    #         tt = obj.read_typetree()
#    #         if "m_GameObject" in tt:
#    #             raise Exception(f"m_GameObject not remapped in {obj.type.name}")
#    #     except Exception:
#    #         e = Exception(f"failed to read typetree in {obj.type.name}")
#    #         print(e)


def remap_path_ids(bcab):
    print(bcab)


def merge(a, bcab):
    acab = a.files["CAB-3fbce1cb6d9f915253e8f713c155c6b3"]

    # print("b sharedassets", bcab_sa.objects)

    # for id, obj in bcab_sa.objects.items():
    #     print(obj, id)

    # prune(bcab, ["A1_S1_GameLevel/Room/A1_S1_Tutorial_Logic/StealthGameMonster_Minion_Tutorial1"])
    # print(len(bcab.objects))

    abundle = acab.objects.pop(1)

    # check disjointness
    for b_pathid in bcab.objects:
        for a_pathid in acab.objects:
            if b_pathid == a_pathid:
                raise Exception(f"duplicate pathid {a_pathid}")
    if 1 in bcab.objects:
        print(f"overwritten by assetbundle: {bcab.objects[1].read()}")

    # merge types
    typelen_a = len(acab.types)
    acab.types = acab.types + bcab.types

    for path_id, obj in bcab.objects.items():
        obj.type_id += typelen_a

    # merge containers
    a_container = abundle.read().read_typetree()["m_Container"]
    b_container = []
    for obj in get_root_objs2(bcab.objects):
        if obj.path_id == 1:
            print("root obj 1 not included")
            continue

        name = obj.name.lower().replace(" ", "_")
        b_container.append(
            (
                f"assets/prefabs/{name}.prefab",
                {"preloadIndex": 0, "preloadSize": 2, "asset": {"m_FileID": 0, "m_PathID": obj.path_id}},
            )
        )
    mergedcontainers = a_container + b_container
    for container in mergedcontainers:
        container[1]["preloadIndex"] = 0
        container[1]["preloadSize"] = 0

    # construct AssetBundle
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

    # merge objects

    i = 0
    mergedobjects = copy.copy(acab.objects)
    for path_id, obj in bcab.objects.items():
        assert path_id == obj.path_id
        assert acab.types[obj.type_id] is not None

        if obj.type == ClassIDType.MonoBehaviour:
            i += 1
            tt = obj.read_typetree()
            tt["m_Script"]["m_FileID"] = 0
            tt["m_Script"]["m_PathID"] = 1
            obj.save_typetree(tt)
            pass
            # continue

        mergedobjects[path_id] = obj

    mergedobjects[1] = abundle
    acab.objects = dict(sorted(mergedobjects.items()))

    acab._enable_type_tree = bcab._enable_type_tree

    files = {
        "CAB-e6a343b52d62193e5ce6de9be1c1fdb2": acab,
    }

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


a = load_bundle("/home/jakob/dev/games/unity/TestAssetBundles/Assets/AssetBundles/prefab_a")
b = load_bundle("/home/jakob/dev/games/unity/TestAssetBundles/Assets/AssetBundles/scenetomerge")
ccab = load_bundle("/home/jakob/.local/share/Steam/steamapps/common/Nine Sols/NineSols_Data/level8")

assert len(b.files) == 2
b_name, b_name_sa = sorted(list(b.files))
assert b_name_sa.endswith(".sharedAssets")
bcab = b.files[b_name]
bcab_sa = b.files[b_name_sa]
bcab_sa_types = sorted(set(x.type.name for x in bcab_sa.objects.values()))
# print("bcab sa types", bcab_sa_types)
# print("ccab types", sorted(set(x.type.name for x in ccab.objects.values())))

merged = merge(a, ccab)

out_path = Path("out/outbundle_merged")
if __name__ == "__main__":
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "wb") as f:
        f.write(merged.save())

    print("-- BUNDLE --")
    sanity = load_bundle(out_path)
    print(sanity.files)
    for cab_name, cab in sanity.files.items():
        if cab_name.endswith(".resS"):
            continue

        for obj in cab.objects.values():
            if obj.type == ClassIDType.MonoBehaviour:
                print(obj.read().m_Script.__dict__)


# scene file: references file_id: 1, path_id something
# scene asset bundle:    file_id: 1, stored in .sharedAssets
# prefa asset bundle:    file_id: 0, stored inline
