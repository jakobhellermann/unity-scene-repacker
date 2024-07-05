from UnityPy.environment import Environment
from pathlib import Path
import json

from repack import repack_scene_bundle
from prune import prune, get_root_objs

scene_map = json.load(open("in/scenes.json", "r"))
monster_preloads = json.load(open("in/monsters.json", "r"))
# monster_preloads = {key: monster_preloads[key] for key in monster_preloads if key in keys}


out_path = Path("out/outbundle_filtered")
project = Path("/home/jakob/.local/share/Steam/steamapps/common/Nine Sols/NineSols_Data")

level_names = [name for name, _ in monster_preloads.items()]
levels = [f"level{scene_map[name]}" for name in level_names]
paths = [str(project.joinpath(level)) for level in levels]

env = Environment(*paths)
serialized_files = [env.files[path] for path in paths]

for level_name, file in zip(level_names, serialized_files):
    level_monsters = monster_preloads[level_name]

    prune(file, level_monsters)

new_bundle = repack_scene_bundle(dict(zip(level_names, serialized_files)))


if __name__ == "__main__":
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "wb") as f:
        f.write(new_bundle.save())

    sanity = Environment(str(out_path)).file
    for name, file in sanity.files.items():
        for go in get_root_objs(file):
            print(name, go.name)
