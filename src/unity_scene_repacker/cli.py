from UnityPy.enums import ClassIDType
from UnityPy.environment import Environment
from pathlib import Path
import json
import argparse

from repack import repack_scene_bundle
from prune import prune
from utils import get_root_objects, get_root_object_readers, get_scene_names

parser = argparse.ArgumentParser()
parser.add_argument("--game-dir", help="game directory where the levels are, i.e. Game/Game_Data", required=True)
parser.add_argument(
    "--preloads",
    help="path to json file, containg map from scene name to list of gameobject paths to include in the assetbundle",
    required=True,
)
parser.add_argument(
    "-o",
    "--output",
    help="path to json file, containg map from scene name to list of gameobject paths to include in the assetbundle",
    default="preloads.bundle",
)
parser.add_argument("--disable", action=argparse.BooleanOptionalAction, default=True)
args = parser.parse_args()

monster_preloads = json.load(open(args.preloads, "r"))

out_path = Path(args.output)
project = Path(args.game_dir)

env = Environment()
ggm = env.load_file(str(project.joinpath("globalgamemanagers")))

scene_map = {name: i for i, name in enumerate(get_scene_names(ggm))}
level_names = monster_preloads.keys()
paths = [str(project.joinpath(f"level{scene_map[name]}")) for name in level_names]

for i, (path, name) in enumerate(zip(paths, level_names)):
    print(f"Loading {i + 1}/{len(paths)} [{name}]                     ", end="\r")
    env.load_file(path)
print()
serialized_files = [env.files[path] for path in paths]


def rename(name: str) -> str:
    name, *rest = name.split(" (")
    return name


objectCountBefore = sum(len(x.objects.values()) for x in serialized_files)

for i, (file, level_name) in enumerate(zip(serialized_files, level_names)):
    print(f"Pruning {i + 1}/{len(paths)} [{level_name}]                     ", end="\r")
    level_monsters = monster_preloads[level_name]

    pruned = prune(file, level_monsters, [ClassIDType.RenderSettings])

    for obj in get_root_object_readers(file):
        tt = obj.read_typetree()
        tt["m_Name"] = rename(tt["m_Name"])
        if args.disable:
            tt["m_IsActive"] = False
        obj.save_typetree(tt)
print()

objectCountAfter = sum(len(x.objects.values()) for x in serialized_files)
print(f"Pruned {objectCountBefore} -> {objectCountAfter} objects")

prefix = "bundle"
new_bundle = repack_scene_bundle(dict(zip([f"{prefix}_{name}" for name in level_names], serialized_files)))

out_path.parent.mkdir(parents=True, exist_ok=True)
with open(out_path, "wb") as f:
    f.write(new_bundle.save())

    print("All included objects:")
    for name, file in Environment(str(out_path)).file.files.items():
        if name.endswith("sharedAssets"):
            continue

        for root in get_root_objects(file):
            print(f"- '{root.m_Name}'")
