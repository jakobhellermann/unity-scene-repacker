import UnityPy
from UnityPy.environment import Environment
from pathlib import Path
import json
import argparse

from repack import repack_scene_bundle
from prune import prune
from utils import get_root_objects, get_root_object_readers

parser = argparse.ArgumentParser()
parser.add_argument("--game-dir", help="game directory where the levels are, i.e. Game/Game_Data", required=True)
parser.add_argument(
    "--scene-defs", help="path to json file, containing a map from scene name to build index", required=True
)
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
parser.add_argument(
    "--disable",
    action=argparse.BooleanOptionalAction,
)
args = parser.parse_args()

scene_map = json.load(open(args.scene_defs, "r"))
monster_preloads = json.load(open(args.preloads, "r"))
# keys = ["A9_S1_Remake_4wei", "A0_S4_tutorial"]
# monster_preloads = {key: monster_preloads[key] for key in monster_preloads if key in keys}

out_path = Path(args.output)
project = Path(args.game_dir)

level_names = [name for name, _ in monster_preloads.items()]
paths = [str(project.joinpath(f"level{scene_map[name]}")) for name in level_names]

env = Environment()
for i, (path, name) in enumerate(zip(paths, level_names)):
    print(f"Loading {i+1}/{len(paths)} [{name}]                     ", end="\r")
    env.load_file(path)
print()
serialized_files = [env.files[path] for path in paths]


def rename(name: str) -> str:
    name, *rest = name.split(" (")
    print(name, rest)
    return name


for level_name, file in zip(level_names, serialized_files):
    print(f"Pruning {i+1}/{len(paths)} [{name}]                     ", end="\r")
    level_monsters = monster_preloads[level_name]
    prune(file, level_monsters)

    for obj in get_root_object_readers(file):
        tt = obj.read_typetree()
        tt["m_Name"] = rename(tt["m_Name"])
        if args.disable:
            tt["m_IsActive"] = False
        obj.save_typetree(tt)


new_bundle = repack_scene_bundle(dict(zip(level_names, serialized_files)))

out_path.parent.mkdir(parents=True, exist_ok=True)
with open(out_path, "wb") as f:
    f.write(new_bundle.save())

    for name, file in Environment(str(out_path)).file.files.items():
        if name.endswith("sharedAssets"):
            continue

        for root in get_root_objects(file):
            print(root.name)
