import os.path

from UnityPy.enums import ClassIDType
from UnityPy.environment import Environment
from pathlib import Path
import json
import argparse

from unity_scene_repacker.repack import repack_scene_bundle
from unity_scene_repacker.prune import prune
from unity_scene_repacker.utils import get_root_objects, get_root_object_readers, get_scene_names


def rename(name: str) -> str:
    name, *rest = name.split(" (")
    return name


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--game-dir", help="game directory where the levels are, i.e. Game/Game_Data", required=True)
    parser.add_argument(
        "--objects",
        help="path to json file, containg map from scene name to list of gameobject paths to include in the assetbundle",
        required=True,
    )
    parser.add_argument(
        "-o",
        "--output",
        help="path to json file, containg map from scene name to list of gameobject paths to include in the assetbundle",
        default="out.unity3d",
    )
    parser.add_argument("--disable", action=argparse.BooleanOptionalAction, default=True)
    args = parser.parse_args()

    scene_objects = json.load(open(args.objects, "r"))

    out_path = Path(args.output)
    project = Path(args.game_dir)

    env = Environment()
    ggm = env.load_file(str(project.joinpath("globalgamemanagers")))

    scene_map = {name: i for i, name in enumerate(get_scene_names(ggm))}
    scene_names = scene_objects.keys()
    paths = [str(project.joinpath(f"level{scene_map[name]}")) for name in scene_names]

    for i, (path, name) in enumerate(zip(paths, scene_names)):
        print(f"Loading {i + 1}/{len(paths)} [{name}]                     ", end="\r")
        env.load_file(path)
    print()
    serialized_files = [env.files[path] for path in paths]

    object_count_before = sum(len(x.objects.values()) for x in serialized_files)

    for i, (file, level_name) in enumerate(zip(serialized_files, scene_names)):
        print(f"Pruning {i + 1}/{len(paths)} [{level_name}]                     ", end="\r")
        level_monsters = scene_objects[level_name]

        prune(file, level_monsters, [ClassIDType.RenderSettings])

        for obj in get_root_object_readers(file):
            tt = obj.read_typetree()
            tt["m_Name"] = rename(tt["m_Name"])
            if args.disable:
                tt["m_IsActive"] = False
            obj.save_typetree(tt)
    print()

    object_count_after = sum(len(x.objects.values()) for x in serialized_files)
    print(f"Pruned {object_count_before} -> {object_count_after} objects")

    prefix = "bundle"
    new_bundle = repack_scene_bundle(dict(zip([f"{prefix}_{name}" for name in scene_names], serialized_files)))

    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "wb") as f:
        f.write(new_bundle.save())
        f.flush()

        size_bytes = os.path.getsize(f.name)
        size_mb = size_bytes / (1024 * 1024)

        print()
        print(f"{out_path} ({size_mb:.2f}MiB):")
        for name, file in Environment(str(out_path)).file.files.items():
            if name.endswith("sharedAssets"):
                continue

            for root in get_root_objects(file):
                print(f"- '{root.m_Name}'")


if __name__ == "__main__":
    main()
