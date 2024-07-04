from UnityPy.environment import Environment
from UnityPy.classes import GameObject, Transform, BuildSettings
from UnityPy.files import SerializedFile
from pathlib import Path
from collections.abc import Iterator
import json
from os import path

project = Path("/home/jakob/.local/share/Steam/steamapps/common/Nine Sols/NineSols_Data")


env = Environment(str(project.joinpath("globalgamemanagers"))).file

for obj in env.objects.values():
    if obj.class_id == 141:
        build_settings: BuildSettings = obj.read()
        print(json.dumps(build_settings.levels))


# levels = []
# for x in project.iterdir():
#     if not x.name.startswith("level"):
#         continue
#
#     levels.append(x)
#
#
# def get_root_objects(file: SerializedFile) -> Iterator[GameObject]:
#     for obj in file.objects.values():
#         if obj.class_id == 4:
#             transform: Transform = obj.read()
#             parent = transform.m_Father.get_obj()
#             if parent is None:
#                 yield transform.m_GameObject.read()
#
#
# association = {}
# env = Environment()
# for levelpath in levels:
#     name = str(levelpath)
#     env.load_file(name)
#     file = env.files[name]
#
#     level = path.basename(name)
#     print(level)
#
#     for go in file.objects.values():
#         if go.class_id == 141:
#             print(type(go))
#
#     break
#     # for go in get_root_objects(file):
#     # if go.name.startswith("A"):
#     # print(go.name)
#     # # print(f"  {go.name}")
#     # # gamelevel = go.name.removesuffix("_GameLevel")
#     # # if gamelevel is not go.name:
#     # #     print(f"{gamelevel} in {name}")
#     # #     association[gamelevel] = level
#
# print(json.dumps(association))
