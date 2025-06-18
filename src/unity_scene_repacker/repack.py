import UnityPy
from UnityPy.environment import SerializedFile
from UnityPy.files import BundleFile, ObjectReader
from UnityPy.enums import ArchiveFlags
from UnityPy.classes import AssetBundle
import copy

from unity_scene_repacker.utils import Fake
from typing import cast

import importlib.resources


def repack_scene_bundle(scenes: dict[str, SerializedFile]) -> BundleFile:
    with importlib.resources.open_binary("unity_scene_repacker.data", "empty_scene_bundle.unity3d") as f:
        emptybundle_bin = f.read()

    emptybundle: BundleFile = UnityPy.load(emptybundle_bin).file
    shared_assets: SerializedFile = emptybundle.files["BuildPlayer-EmptyScene.sharedAssets"]

    assetbundle_meta: ObjectReader[AssetBundle] = shared_assets.objects[2]
    assetbundle_meta.save_typetree(
        {
            "m_Name": "scenebundle",
            "m_PreloadTable": [],
            "m_Container": [
                (
                    f"Assets/SceneBundle/{name}.unity",
                    {
                        "preloadIndex": 0,
                        "preloadSize": 0,
                        "asset": {"m_FileID": 0, "m_PathID": 0},
                    },
                )
                for name in scenes
            ],
            "m_MainAsset": {
                "preloadIndex": 0,
                "preloadSize": 0,
                "asset": {"m_FileID": 0, "m_PathID": 0},
            },
            "m_RuntimeCompatibility": 1,
            "m_AssetBundleName": "",
            "m_Dependencies": [],
            "m_IsStreamedSceneAssetBundle": True,
            "m_ExplicitDataLayout": 0,
            "m_PathFlags": 7,
            "m_SceneHashes": [],
        }
    )

    files = {}
    first = True
    for name, scene in scenes.items():
        scene_shared_assets = shared_assets
        if not first:
            scene_shared_assets = copy.copy(shared_assets)
            scene_shared_assets.objects = copy.copy(scene_shared_assets.objects)
            assert scene_shared_assets.objects.pop(2).class_id == 142  # AssetBundle
        first = False

        scene.flags = 4
        files[f"BuildPlayer-{name}.sharedAssets"] = scene_shared_assets
        files[f"BuildPlayer-{name}"] = scene

    return cast(
        BundleFile,
        Fake(
            BundleFile,
            signature="UnityFS",
            version=7,
            version_player="5.x.x",
            version_engine="2020.2.2f1",
            dataflags=ArchiveFlags.BlocksAndDirectoryInfoCombined | 3,
            _block_info_flags=65,
            _uses_block_alignment=True,
            files=files,
        ),
    )
