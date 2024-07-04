from UnityPy.environment import Environment
from pathlib import Path
import json

from repack import repack_scene_bundle
from prune import prune, get_root_objs

scene_map = json.load(open("in/scenes.json", "r"))
monsters = [
    ("A9_S1_Remake_4wei", "A9_S1/Room/Prefab/A9_MonsterCandidate/StealthGameMonster_GunBoyElite"),
    # (
    #    "A10_S5_Boss_Jee",
    #    "A10S5/Room/Boss And Environment Binder/General Boss Fight FSM Object 姬 Variant/FSM Animator/LogicRoot/---Boss---/BossShowHealthArea/StealthGameMonster_Boss_Jee",
    # ),
]


out_path = Path("out/outbundle_filtered")
project = Path("/home/jakob/.local/share/Steam/steamapps/common/Nine Sols/NineSols_Data")

level_names = [name for name, _ in monsters]
levels = [f"level{scene_map[name]}" for name, _ in monsters]
paths = [str(project.joinpath(level)) for level in levels]

env = Environment(*paths)
serialized_files = [env.files[path] for path in paths]

for level_name, file in zip(level_names, serialized_files):
    level_monsters = [path for scene, path in monsters if scene == level_name]
    prune(file, level_monsters)

new_bundle = repack_scene_bundle(dict(zip(level_names, serialized_files)))


if __name__ == "__main__":
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "wb") as f:
        f.write(new_bundle.save())

    sanity = Environment(str(out_path)).file
    s = sanity.files["BuildPlayer-A9_S1_Remake_4wei"]

    for go in get_root_objs(s):
        print(go.m_Transform.read().m_Father.__dict__)
