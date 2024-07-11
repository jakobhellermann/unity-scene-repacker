from src.utils import load_scenes, lookup_path, get_root_objects, components_in_children, path
from UnityPy.enums import ClassIDType
from UnityPy.classes import MonoBehaviour
from pathlib import Path
import json

project = Path("/home/jakob/.local/share/Steam/steamapps/common/Nine Sols/NineSols_Data/")

scene_map = json.load(open("ninesols/scenes.json", "r"))
monster_preloads = json.load(open("ninesols/monsters.json", "r"))

keys = ["A0_S4_tutorial"]
monster_preloads = {key: monster_preloads[key] for key in monster_preloads if key in keys}
level_names = [name for name, _ in monster_preloads.items()]

serialized_files = load_scenes(project, level_names, scene_map)


for level_name, scene in zip(level_names, serialized_files):
    level_monsters = monster_preloads[level_name]
    print(level_name, level_monsters)

    root_objs = list(get_root_objects(scene))

    for monster in level_monsters:
        monster = lookup_path(monster, root_objs).m_GameObject.read()

        for comp in components_in_children(monster, type=ClassIDType.MonoBehaviour):
            x: MonoBehaviour = comp.read()
            # print(x.m_Script.read().name)
            if x.m_Script.read().name == "LootSpawner":
                print(path(x.m_GameObject.read()))
                print(x.serialized_type.__dict__)
            # if x.m_Script.read().name == "PickItemAction":
