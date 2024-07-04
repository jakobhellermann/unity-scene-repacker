import random
import os
from os import path

import UnityPy
from UnityPy.classes import MonoBehaviour, MonoScript, GameObject, AssetBundle, PPtr
from UnityPy.files import BundleFile, ObjectReader
from UnityPy.files.SerializedFile import SerializedFile, SerializedType
from UnityPy.helpers import TypeTreeHelper

TypeTreeHelper.read_typetree_c = False

class Fake(object):
    """
    fake class for easy class creation without init call
    """

    def __init__(self, **kwargs):
        self.__dict__.update(kwargs)
        if "_class" in kwargs:
            self.__class__ = kwargs["_class"]

    def save(self):
        return self.data



dir = "/home/jakob/dev/games/unity/TestAssetBundles/Assets/"
bundle_path = path.join(dir, "AssetBundles/prefabbundle")

project = "/home/jakob/.local/share/Steam/steamapps/common/Nine Sols/NineSols_Data"
level_path = path.join(project, "level8")


def read_level(level: str):
    # os.chdir("/home/jakob/.local/share/Steam/steamapps/common/Nine Sols/NineSols_Data")
    # env = Environment()
    # env.load_file(level)
    # file: SerializedFile = list(env.files.values())[0]

    env = UnityPy.load(level)
    file: SerializedFile = env.file

    for obj in file.objects.values():
        if obj.type == 1:
            obj: GameObject = obj.read()
            if obj.name == "StealthGameMonster_GunBoy":
                # x = map(lambda x: x.read(), filter(lambda c: c.type == 114, obj.m_Components))

                # print(obj.m_Components[-1].get_obj().read().m_GameObject)

                x = map(lambda x: x.get_obj(), filter(lambda c: c.type == 114, obj.m_Components))
                return list(x)


        # if(obj.type != 114): continue

        # obj = obj.read()
        # if(isinstance(obj, MonoBehaviour)):

        #     go = obj.m_GameObject.get_obj()
        #     if go is None: continue

        #     if "Gun" in go.read().name:
        #         print(go.read().name)

def generate_path_id(objects):
    while True:
        uid = random.randint(-(2 ** 16), 2 ** 16 - 1)
        if uid not in objects:
            return uid

new_path_id = generate_path_id([])
print(new_path_id)

def add_thing(to: UnityPy.Environment, assets_file: SerializedFile, object_reader: ObjectReader):
    file_id = 0
    path_id = new_path_id

    fakeobj = list(assets_file.objects.values())[0]
    # print(fakeobj)
    # assets_file.objects[new_path_id] = fakeobj


    found_type_id = None
    for i, ty in enumerate(assets_file.types):
        if ty.class_id == 114:
            found_type_id = i
            break
    assert found_type_id is not None

    object_reader.path_id = path_id
    object_reader.type_id = found_type_id

    assets_file.objects[path_id] = object_reader

    # assert assets_file._enable_type_tree == object_reader.assets_file._enable_type_tree
    # newtypeid=len(assets_file.types)
    # assets_file.types.append(object_reader.assets_file.types[object_reader.type_id])




def patch_bundle(bundle: str, components: list[ObjectReader]):
    env = UnityPy.load(bundle)
    bundle: BundleFile = env.file
    cab: SerializedFile = next(bundle[name] for name in bundle.files if name.startswith("CAB"))


    fakeobj = list(cab.objects.values())[4]
    patchedcomponent=fakeobj

    # patchedcomponent = components[-6]
    add_thing(env, cab, patchedcomponent)

    for key in cab.objects:
        obj = cab.objects[key].read()

        if(isinstance(obj, GameObject)) and obj.name == "Circle" and False:
            circlego = obj.m_Components[-1].read_typetree()["m_GameObject"]

            pptr = PPtr.__new__(PPtr)
            pptr.path_id = new_path_id
            pptr.file_id = 0
            pptr.assets_file = cab
            pptr._obj = None


            x: ObjectReader = pptr.get_obj()
            tt = x.read_typetree()
            tt["m_GameObject"] = circlego
            x.save_typetree(tt)

            obj.m_Components.append(pptr)

    return bundle.save()



components = read_level(level_path)

bundle_bytes = patch_bundle(bundle_path, components)
with open("out/outbundle", "wb") as f:
    f.write(bundle_bytes)


envnew = UnityPy.load("out/outbundle")
# print(envnew.objects[-1].read())


