import json
from pathlib import Path
from sys import platform

from UnityPy import Environment
from UnityPy.classes import MonoBehaviour
from UnityPy.enums import ClassIDType
from UnityPy.files import SerializedFile

from unity_scene_repacker import utils

steam = Path("C:/Program Files (x86)/Steam/steamapps/common")
if platform == "linux" or platform == "linux2":
    steam = Path("/mnt/c/Program Files (x86)/Steam/steamapps/common")

path = steam.joinpath("Hollow Knight/hollow_knight_Data")
script = "TransitionPoint"
outfile = "out/transitions.json"

env = Environment()
env.path = str(path)
ggm = env.load_file(str(path.joinpath("globalgamemanagers")))

scenes = utils.get_scene_names(ggm)

print(json.dumps(dict(enumerate(scenes)), indent=4))

all_transitions = {}

outcs = ""

for i, scene_name in enumerate(scenes):
    scene_file: SerializedFile = env.load_file(str(path.joinpath(f"level{i}")))
    print(i, scene_name)

    transitions = []
    for obj in scene_file.objects.values():
        if obj.type == ClassIDType.MonoBehaviour:
            mb: MonoBehaviour = obj.read(check_read=False)
            script = mb.m_Script.read()
            if script.m_Name == "TransitionPoint":
                transitions.append(mb.m_GameObject.read().m_Name)

    outcs += f"""        {{ "{scene_name}", [{",".join(f'"{x}"' for x in transitions)}] }},\n"""
    all_transitions[scene_name] = transitions

print(outcs)

with open(outfile, "w") as f:
    f.write(json.dumps(all_transitions, indent=4))
